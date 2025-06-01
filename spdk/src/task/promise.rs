#![allow(dead_code)]
use std::{
    fmt::Debug,
    mem::{self, MaybeUninit},
    pin::Pin,
    ptr::addr_of_mut,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use futures::Future;
use libc::c_void;

use crate::{
    errors::{Errno, EINVAL},
    thread::Thread,
    to_result,
};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Encapsulates the state of a [`Promise`].
///
/// The following state diagram shows the state transitions of a [`Promise`].
///
/// ```mermaid
/// stateDiagram-v2
/// Empty --> Requesting : poll(cx)
/// Requesting --> Waiting : poll(cx)
/// Requesting --> Kept : set_result(res) | Ready(res)
/// Waiting --> Waiting : poll(cx)
/// Waiting --> Kept : set_result(res)
/// Kept --> Fulfilled : poll(cx)
/// ```
#[derive(Debug, Default)]
enum PromiseState<T>
where
    T: Debug + 'static,
{
    /// The initial state of a promise.
    #[default]
    Empty,

    /// The promisee is requesting a promise.
    ///
    /// This state indicates the promise is invoking the function to start the
    /// asynchronous operation.
    Requesting,

    /// The promisee is waiting for the promise to be kept.
    ///
    /// This state occurs when the start function returns `Poll::Pending` and
    /// before a completion callback is invoked.
    Waiting(Waker),

    /// The promisor has kept the promise and delivered a result.
    ///
    /// This state occurs either when the start function returns
    /// `Poll::Ready(res)` or when the asynchrounous operation invokes a
    /// completion callback.
    ///
    /// If a completion function is invoked before the start function returns,
    /// the promise transitions from the `Requesting` state to the `Kept` state
    /// directly. Otherwise, the promise transitions from the `Requesting` state
    /// to the `Waiting` state.
    Kept(Result<T, Errno>),

    /// The promisee has received the promised result.
    ///
    /// This state occurs when the result has been returned to the promise's
    /// caller.
    Fulfilled,
}

unsafe impl<T> Send for PromiseState<T> where T: Debug + Send + 'static {}

impl<T> PromiseState<T>
where
    T: Debug + 'static,
{
    /// Replaces the current state with the specified value and returns the old state.
    fn replace(&mut self, value: Self) -> Self {
        mem::replace(&mut *self, value)
    }

    /// Polls the state of the operation, advancing to the next state if possible.
    fn poll(&mut self, cx: &Context<'_>) -> Self {
        match self {
            Self::Empty => self.replace(Self::Requesting),
            Self::Requesting => self.replace(Self::Waiting(cx.waker().clone())),
            Self::Waiting(waker) => Self::Waiting(waker.clone()),
            Self::Kept(_) => self.replace(Self::Fulfilled),
            _ => panic!("promise polled in unexpected state: {:?}", self),
        }
    }
}

/// Receives the result of an asynchronous operation.
pub struct Promissory<R, C = ()>
where
    R: Debug + 'static,
    C: 'static,
{
    state: Mutex<PromiseState<R>>,
    ctx: C,
}

impl<R, C> Promissory<R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    /// Returns a new `Arc<Promise>` instance with the provided context.
    pub fn with_context(ctx: C) -> Arc<Self> {
        Arc::new(Self {
            state: Default::default(),
            ctx,
        })
    }

    /// Returns a new `Arc<Promise>` instance, initializing the context in-place
    /// using the specified function.
    pub fn with_context_in_place<F>(ctx_init_fn: F) -> Arc<Self>
    where
        F: FnOnce(*mut C),
    {
        let arc_ctx = Arc::new(MaybeUninit::<Self>::uninit());

        unsafe {
            let ctx = (*(Arc::into_raw(arc_ctx) as *mut MaybeUninit<Self>)).as_mut_ptr();

            addr_of_mut!((*ctx).state).write(Default::default());
            ctx_init_fn(addr_of_mut!((*ctx).ctx));

            Arc::from_raw(ctx)
        }
    }

    /// Polls the state of the operation, advancing to the next state if possible.
    fn poll(&self, cx: &Context<'_>) -> PromiseState<R> {
        self.state.lock().unwrap().poll(cx)
    }

    /// Returns a reference to the user context initialized using either the
    /// [`Promise::with_context`] or [`Promise::with_context_in_place`]
    /// functions.
    pub fn user_context(arc_self: &Arc<Self>) -> &C {
        &arc_self.ctx
    }

    /// Returns a mutable reference to the user context if there are no other
    /// `Arc` or `Weak` pointers to the same allocation.
    ///
    /// Returns [`None`] otherwise because it is not safe to mutate the context
    /// of a shared valued.
    ///
    /// The user context is initialized using either the
    /// [`Promise::with_context`] or [`Promise::with_context_in_place`]
    /// functions.
    pub fn user_context_mut(arc_self: &mut Arc<Self>) -> Option<&mut C> {
        Arc::get_mut(arc_self).map(|p| &mut p.ctx)
    }

    /// Sets the result of the operation and awakens the [`Promise`] awaiting the result.
    pub fn set_result(arc_self: Arc<Self>, res: Result<R, Errno>) {
        let prev_state = match arc_self.state.try_lock() {
            Ok(mut state) => Ok(state.replace(PromiseState::Kept(res))),
            Err(_) => Err(res),
        };

        match prev_state {
            Ok(prev_state) => match prev_state {
                PromiseState::Requesting => (),
                PromiseState::Waiting(waker) => waker.wake(),
                _ => panic!("promise kept in unexpected state: {:?}", prev_state),
            },
            Err(res) => {
                Thread::current()
                    .send_msg(move || {
                        Self::set_result(arc_self, res);
                    })
                    .expect("send result");
            }
        }
    }

    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a raw pointer to an SPDK object of type `R` and
    /// converts it to the appropriate Rust type `T`. If the received pointer is
    /// null, the result is a suitable `Err(Errno)` value.
    unsafe extern "C" fn complete_with_object<T>(cx: *mut c_void, obj: *mut T)
    where
        R: Debug + TryFrom<*mut T, Error = Errno> + 'static,
        C: 'static,
    {
        let arc_self = Self::from_raw(cx.cast());

        Self::set_result(arc_self, obj.try_into().map_err(|_| EINVAL));
    }

    /// Returns a pointer to a callback function and a context pointer suitable
    /// for passing to an asynchronous function call. The callback is invoked to
    /// set the result of a [`Promise`].
    ///
    /// This callback receives a raw pointer to an SPDK object of type `R` and
    /// converts it to the appropriate Rust type `T`. If the received pointer is
    /// null, the result is a suitable `Err(Errno)` value.
    pub fn callback_with_object<T>(
        arc_self: &Arc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void, *mut T), *const Self)
    where
        R: Debug + TryFrom<*mut T, Error = Errno> + 'static,
    {
        (Self::complete_with_object, Self::into_raw(arc_self.clone()))
    }

    /// Consumes an `Arc<Promissory>` instance and returns the wrapped pointer.
    ///
    /// To avoid a memory leak, the pointer must be converted back to an
    /// `Arc<Promissory>` using the [`Promissory::from_raw`] function.
    pub fn into_raw(arc_self: Arc<Self>) -> *const Self {
        Arc::into_raw(arc_self)
    }

    /// Constructs an `Arc<Promissory>` instance from a raw pointer.
    ///
    /// The raw pointer must have been previously returned by a call to
    /// `Promissory<R, C>::into_raw`.
    ///
    /// # Safety
    ///
    /// See [`Arc<T>::from_raw`] for safety requirements.
    pub unsafe fn from_raw(raw: *const Self) -> Arc<Self> {
        Arc::from_raw(raw)
    }
}

impl<C> Promissory<(), C> {
    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a status code. If the status code is 0, the result is
    /// `Ok(())`. Otherwise, the status is converted to a suitable `Err(Errno)`
    /// value.
    unsafe extern "C" fn complete_with_status(cx: *mut c_void, status: i32) {
        let arc_self = Self::from_raw(cx.cast());

        Self::set_result(arc_self, to_result!(status));
    }

    /// Returns a pointer to a callback function and a context pointer suitable
    /// for passing to an asynchronous function call. The callback is invoked to
    /// set the result of a [`Promise`].
    ///
    /// This callback receives a status code. If the status code is 0, the result is
    /// `Ok(())`. Otherwise, the status is converted to a suitable `Err(Errno)`
    /// value.
    pub fn callback_with_status(
        arc_self: &Arc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void, i32), *const Self) {
        (Self::complete_with_status, Self::into_raw(arc_self.clone()))
    }

    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback always sets the result to `Ok(())`.
    unsafe extern "C" fn complete_with_ok(cx: *mut c_void) {
        let arc_self = Self::from_raw(cx.cast());

        Self::set_result(arc_self, Ok(()));
    }

    /// Returns a pointer to a callback function and a context pointer suitable
    /// for passing to an asynchronous function call. The callback is invoked to
    /// set the result of a [`Promise`].
    ///
    /// This callback always sets the result to `Ok(())`.
    pub fn callback_with_ok(
        arc_self: &Arc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void), *const Self) {
        (Self::complete_with_ok, Self::into_raw(arc_self.clone()))
    }
}

impl<R, C> Default for Promissory<R, C>
where
    R: Debug + 'static,
    C: Default + 'static,
{
    fn default() -> Self {
        Self {
            state: Default::default(),
            ctx: Default::default(),
        }
    }
}

/// A function that starts the asynchronous operation the will yield a result
/// for the [`Promise`].
type StartFn<'a, R, C> = dyn FnOnce(&mut Arc<Promissory<R, C>>) -> Poll<Result<R, Errno>> + 'a;

/// Orchestrates the execution of an asynchronous operation and provides access
/// to its result.
pub struct Promise<'a, R, C = ()>
where
    R: Debug + 'static,
    C: 'static,
{
    start_fn: Option<Box<StartFn<'a, R, C>>>,
    promissory: Arc<Promissory<R, C>>,
}

unsafe impl<'a, R, C> Send for Promise<'a, R, C>
where
    R: Debug + Send + 'static,
    C: 'static,
{
}

impl<'a, R> Promise<'a, R>
where
    R: Debug + 'static,
{
    /// Returns a new `Promise` instance.
    ///
    /// The caller provides a function that starts the asynchronous operation
    /// passing one of the `complete_with_*` callbacks and the provided context
    /// pointer to the SPDK API.
    pub fn new<S>(start_fn: S) -> Self
    where
        S: FnOnce(&mut Arc<Promissory<R>>) -> Poll<Result<R, Errno>> + 'a,
    {
        Self {
            start_fn: Some(Box::new(start_fn)),
            promissory: Default::default(),
        }
    }
}

impl<'a, R, C> Promise<'a, R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    /// Returns a new `Promise` instance the user context of the related
    /// `Promissory` initialized to the specified value.
    pub fn with_context<S>(ctx: C, start_fn: S) -> Self
    where
        S: FnOnce(&mut Arc<Promissory<R, C>>) -> Poll<Result<R, Errno>> + 'a,
    {
        Self {
            start_fn: Some(Box::new(start_fn)),
            promissory: Promissory::with_context(ctx),
        }
    }

    /// Returns a new `Promise` instance with the user context of the related
    /// `Promissory` initialized in-place using the specified function.
    ///
    /// The pointer provided to the initialization function points to
    /// uninitialized memory. Reading from this pointer is undefined behavior.
    pub fn with_context_in_place<F, S>(ctx_init_fn: F, start_fn: S) -> Self
    where
        S: FnOnce(&mut Arc<Promissory<R, C>>) -> Poll<Result<R, Errno>> + 'a,
        F: FnOnce(*mut C),
    {
        Self {
            start_fn: Some(Box::new(start_fn)),
            promissory: Promissory::with_context_in_place(ctx_init_fn),
        }
    }
}

impl<'a, R, C> Future for Promise<'a, R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    type Output = Result<R, Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.promissory.poll(cx);

        match state {
            PromiseState::Empty => {
                let start_fn = self
                    .as_mut()
                    .start_fn
                    .take()
                    .expect("called in empty state");

                match (start_fn)(&mut self.promissory) {
                    Poll::Pending => {
                        let state = self.promissory.poll(cx);

                        if let PromiseState::Kept(res) = state {
                            return Poll::Ready(res);
                        }

                        Poll::Pending
                    }
                    Poll::Ready(res) => Poll::Ready(res),
                }
            }
            PromiseState::Waiting(_) => Poll::Pending,
            PromiseState::Kept(res) => Poll::Ready(res),
            _ => unreachable!("promise already fulfilled"),
        }
    }
}
