#![allow(dead_code)]
use std::{
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    mem::{self, transmute, MaybeUninit},
    pin::Pin,
    rc::{Rc, Weak},
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
/// Empty --> Requesting : request(start_fn)
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
    /// This state indicates the promise is invoking the function to start the asynchronous
    /// operation.
    Requesting,

    /// The promisee is waiting for the promise to be kept.
    ///
    /// This state occurs when the start function returns `Poll::Pending` and before a completion
    /// callback is invoked.
    Waiting(Waker),

    /// The promisor has kept the promise and delivered a result.
    ///
    /// This state occurs either when the start function returns `Poll::Ready(res)` or when the
    /// asynchrounous operation invokes a completion callback.
    ///
    /// If a completion function is invoked before the start function returns, the promise
    /// transitions from the `Requesting` state to the `Kept` state directly. Otherwise, the promise
    /// transitions from the `Requesting` state to the `Waiting` state.
    Kept(Result<T, Errno>),

    /// The promisee has received the promised result.
    ///
    /// This state occurs when the result has been returned to the promise's caller.
    Fulfilled,
}

unsafe impl<T> Send for PromiseState<T> where T: Debug + Send + 'static {}

impl<T> PromiseState<T>
where
    T: Debug + 'static,
{
    /// Polls the state of the operation, advancing to the next state if possible.
    fn poll(&mut self, cx: &Context<'_>) -> Self {
        match self {
            Self::Requesting => mem::replace(self, Self::Waiting(cx.waker().clone())),
            Self::Waiting(waker) => Self::Waiting(waker.clone()),
            Self::Kept(_) => mem::replace(self, Self::Fulfilled),
            _ => panic!("promise polled in unexpected state: {:?}", self),
        }
    }

    /// Sets the state of the operation to [`Requesting`].
    ///
    /// [`Requesting`]: Self::Requesting
    fn set_requesting(&mut self) {
        match self {
            PromiseState::Empty => *self = Self::Requesting,
            _ => panic!("set_requesting called in unexpected state: {:?}", self),
        }
    }

    /// Sets the state and result of the operation to [`Kept`].
    ///
    /// [`Kept`]: Self::Kept
    fn set_kept(&mut self, res: Result<T, Errno>) -> Self {
        match self {
            Self::Requesting | Self::Waiting(_) => mem::replace(self, Self::Kept(res)),
            _ => panic!("set_kept called in unexpected state: {:?}", self),
        }
    }
}

/// Receives the result of an asynchronous operation.
///
/// <div class="warning">
///
/// The `Promissory` type is not thread-safe. It must be used on the same SPDK thread that created
/// it.
///
/// </div>
pub struct Promissory<R, C = ()>
where
    R: Debug + 'static,
    C: 'static,
{
    state: RefCell<PromiseState<R>>,
    thread: Thread,
    ctx: C,
}

impl<R, C> Promissory<R, C>
where
    R: Debug + 'static,
    C: Unpin + 'static,
{
    /// Returns a new `Rc<Promise>` instance with the provided context.
    pub fn with_context(ctx: C) -> Rc<Self> {
        Rc::new(Self {
            state: Default::default(),
            thread: Thread::current(),
            ctx,
        })
    }

    /// Constructs a new `Rc<Promissory>` instance giving you a `Weak<Promissory>` to the allocation
    /// allowing you to construct the context, C, holding a weak pointer to itself.
    ///
    /// See [`Rc::new_cyclic`] for more information on cyclic reference counting strategies.
    pub fn with_context_cyclic<F>(data_fn: F) -> Rc<Self>
    where
        F: FnOnce(&Weak<Self>) -> C,
    {
        Rc::new_cyclic(|weak_self| Self {
            state: Default::default(),
            thread: Thread::current(),
            ctx: data_fn(weak_self),
        })
    }
}

impl<R, C> Promissory<R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    /// Constructs a new `Rc<Promissory>` instance initializing the context, `C` in place. This
    /// function also provides a `Weak<Promissory>` to the allocation allowing you to initialize the
    /// context holding a weak pointer to itself.
    ///
    /// See [`Rc::new_cyclic`] for more information on cyclic reference counting strategies.
    ///
    /// # Safety
    ///
    /// The initialization function, `init_fn`, must ensure that it initializes the context through
    /// the reference and does not access or mutate it by through the weak pointer.
    pub fn with_context_cyclic_in_place<I>(init_fn: I) -> Rc<Self>
    where
        I: FnOnce(Pin<&mut MaybeUninit<C>>, &Weak<Promissory<R, MaybeUninit<C>>>),
    {
        let this = Rc::new(Promissory::<R, _> {
            state: Default::default(),
            thread: Thread::current(),
            ctx: UnsafeCell::new(MaybeUninit::zeroed()),
        });

        // SAFETY: The promissory has been initialized except its context.
        let weak: Weak<Promissory<R, MaybeUninit<C>>> = unsafe { transmute(Rc::downgrade(&this)) };

        // SAFETY: The initialization function must ensure that it initializes the context through
        // the reference and does not access or mutate it through the weak pointer.
        init_fn(unsafe { Pin::new_unchecked(&mut *this.ctx.get()) }, &weak);

        // SAFETY: The promissory has been completely initialized at this point.
        unsafe { transmute(this) }
    }

    /// Polls the state of the operation, advancing to the next state if possible.
    fn poll_state(&self, cx: &Context<'_>) -> PromiseState<R> {
        self.state.borrow_mut().poll(cx)
    }

    /// Sets the state of the operation to [`PromiseState::Requesting`].
    fn set_requesting(&self) {
        self.state.borrow_mut().set_requesting();
    }

    /// Sets the state and result of the operation to [`PromiseState::Kept`].
    fn set_kept(&self, res: Result<R, Errno>) -> PromiseState<R> {
        self.state.borrow_mut().set_kept(res)
    }

    /// Returns a reference to the user context.
    pub fn user_context(rc_self: &Rc<Self>) -> &C {
        &rc_self.ctx
    }

    /// Returns a mutable reference to the user context if there are no other `Rc` or `Weak`
    /// pointers to the same allocation.
    ///
    /// Returns [`None`] otherwise because it is not safe to mutate the context of a shared valued.
    pub fn user_context_mut(rc_self: &mut Rc<Self>) -> Option<&mut C> {
        Rc::get_mut(rc_self).map(|p| &mut p.ctx)
    }

    /// Sets the result of the operation and awakens the [`Promise`] awaiting the result.
    pub fn set_result(rc_self: Rc<Self>, res: Result<R, Errno>) {
        assert!(
            rc_self.thread.is_current(),
            "set_result called from wrong thread"
        );

        let prev_state = match rc_self.state.try_borrow_mut() {
            Ok(mut state) => Ok(state.set_kept(res)),
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
                        Self::set_result(rc_self, res);
                    })
                    .expect("send result");
            }
        }
    }

    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a raw pointer to an SPDK object of type `R` and converts it to the
    /// appropriate Rust type `T`. If the received pointer is null, the result is a suitable
    /// `Err(Errno)` value.
    unsafe extern "C" fn complete_with_object<T>(cx: *mut c_void, obj: *mut T)
    where
        R: Debug + TryFrom<*mut T, Error = Errno> + 'static,
        C: 'static,
    {
        let rc_self = Self::from_raw(cx.cast());

        Self::set_result(rc_self, obj.try_into().map_err(|_| EINVAL));
    }

    /// Returns a pointer to a callback function and a context pointer suitable for passing to an
    /// asynchronous function call. The callback is invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a raw pointer to an SPDK object of type `R` and converts it to the
    /// appropriate Rust type `T`. If the received pointer is null, the result is a suitable
    /// `Err(Errno)` value.
    pub fn callback_with_object<T>(
        rc_self: &Rc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void, *mut T), *const Self)
    where
        R: Debug + TryFrom<*mut T, Error = Errno> + 'static,
    {
        (Self::complete_with_object, Self::into_raw(rc_self.clone()))
    }

    /// Consumes an `Rc<Promissory>` instance and returns the wrapped pointer.
    ///
    /// To avoid a memory leak, the pointer must be converted back to an `Rc<Promissory>` using the
    /// [`Promissory::from_raw`] function.
    pub fn into_raw(rc_self: Rc<Self>) -> *const Self {
        Rc::into_raw(rc_self)
    }

    /// Constructs an `Rc<Promissory>` instance from a raw pointer.
    ///
    /// The raw pointer must have been previously returned by a call to `Promissory<R,
    /// C>::into_raw`.
    ///
    /// # Safety
    ///
    /// See [`Rc<T>::from_raw`] for safety requirements.
    pub unsafe fn from_raw(raw: *const Self) -> Rc<Self> {
        let rc_self = Rc::from_raw(raw);

        assert!(
            rc_self.thread.is_current(),
            "from_raw called from wrong thread"
        );

        rc_self
    }
}

impl<C> Promissory<(), C> {
    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a status code. If the status code is 0, the result is `Ok(())`.
    /// Otherwise, the status is converted to a suitable `Err(Errno)` value.
    unsafe extern "C" fn complete_with_status(cx: *mut c_void, status: i32) {
        let rc_self = Self::from_raw(cx.cast());

        Self::set_result(rc_self, to_result!(status));
    }

    /// Returns a pointer to a callback function and a context pointer suitable for passing to an
    /// asynchronous function call. The callback is invoked to set the result of a [`Promise`].
    ///
    /// This callback receives a status code. If the status code is 0, the result is `Ok(())`.
    /// Otherwise, the status is converted to a suitable `Err(Errno)` value.
    pub fn callback_with_status(
        rc_self: &Rc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void, i32), *const Self) {
        (Self::complete_with_status, Self::into_raw(rc_self.clone()))
    }

    /// A callback invoked to set the result of a [`Promise`].
    ///
    /// This callback always sets the result to `Ok(())`.
    unsafe extern "C" fn complete_with_ok(cx: *mut c_void) {
        let rc_self = Self::from_raw(cx.cast());

        Self::set_result(rc_self, Ok(()));
    }

    /// Returns a pointer to a callback function and a context pointer suitable for passing to an
    /// asynchronous function call. The callback is invoked to set the result of a [`Promise`].
    ///
    /// This callback always sets the result to `Ok(())`.
    pub fn callback_with_ok(
        rc_self: &Rc<Self>,
    ) -> (unsafe extern "C" fn(*mut c_void), *const Self) {
        (Self::complete_with_ok, Self::into_raw(rc_self.clone()))
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
            thread: Thread::current(),
            ctx: Default::default(),
        }
    }
}

/// A future implementation for awaiting the result of a [`Promise`].
struct FuturePromise<R, C>(Rc<Promissory<R, C>>)
where
    R: Debug + 'static,
    C: 'static;

impl<R, C> Future for FuturePromise<R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    type Output = Result<R, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.0.poll_state(cx);

        match state {
            PromiseState::Requesting | PromiseState::Waiting(_) => Poll::Pending,
            PromiseState::Kept(res) => Poll::Ready(res),
            _ => unreachable!("promise polled in unexpected state: {:?}", state),
        }
    }
}

/// Orchestrates the execution of an asynchronous operation and provides access to its result.
///
/// <div class="warning">
///
/// The `Promise` type is not thread-safe. It must be used on the same SPDK thread that created it.
///
/// </div>
pub struct Promise<R, C = ()>(Rc<Promissory<R, C>>)
where
    R: Debug + 'static,
    C: 'static;

unsafe impl<R, C> Send for Promise<R, C>
where
    R: Debug + Send + 'static,
    C: 'static,
{
}

impl<R> Promise<R>
where
    R: Debug + 'static,
{
    /// Returns a new `Promise` instance.
    ///
    /// A newly created `Promise` is empty. You must call the [`request`] method to begin the
    /// asynchronous operation.
    ///
    /// [`request`]: Self::request
    pub fn new() -> Self {
        Self(Default::default())
    }
}

impl<R> Default for Promise<R>
where
    R: Debug + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<R, C> Promise<R, C>
where
    R: Debug + 'static,
    C: Unpin + 'static,
{
    /// Returns a new `Promise` instance with the user context of the related `Promissory`
    /// initialized to the specified value.
    ///
    /// A newly created `Promise` is empty. You must call the [`request`] method to begin the
    /// asynchronous operation.
    ///
    /// [`request`]: Self::request
    pub fn with_context(ctx: C) -> Self {
        Self(Promissory::with_context(ctx))
    }

    /// Constructs a new `Promise` instance giving you a `Weak<Promissory>` to the related
    /// `Promissory` allocation allowing you to construct the context, C, holding a weak pointer to
    /// it.
    ///
    /// See [`Rc::new_cyclic`] for more information on cyclic reference counting strategies.
    ///
    /// A newly created `Promise` is empty. You must call the [`request`] method to begin the
    /// asynchronous operation.
    ///
    /// [`request`]: Self::request
    pub fn with_context_cyclic<F>(data_fn: F) -> Self
    where
        F: FnOnce(&Weak<Promissory<R, C>>) -> C,
    {
        Self(Promissory::with_context_cyclic(data_fn))
    }
}

impl<R, C> Promise<R, C>
where
    R: Debug + 'static,
    C: 'static,
{
    /// Constructs a new `Promise` instance initializing the context, `C`, in place in the related
    /// `Promissory` allocation. This function also provides a `Weak<Promissory>` to the allocation
    /// allowing you to initialize the context holding a weak pointer to itself.
    ///
    /// See [`Rc::new_cyclic`] for more information on cyclic reference counting strategies.
    ///
    /// A newly created `Promise` is empty. You must call the [`request`] method to begin the
    /// asynchronous operation.
    ///
    /// # Safety
    ///
    /// The initialization function, `init_fn`, must ensure that it initializes the context through
    /// the reference and does not access or mutate it by through the weak pointer.
    ///
    /// [`request`]: Self::request
    pub fn with_context_cyclic_in_place<I>(init_fn: I) -> Self
    where
        I: FnOnce(Pin<&mut MaybeUninit<C>>, &Weak<Promissory<R, MaybeUninit<C>>>),
    {
        Self(Promissory::with_context_cyclic_in_place(init_fn))
    }

    /// Begins the asynchronous operation to fulfill the promise.
    pub fn request<S>(mut self, start_fn: S) -> impl Future<Output = Result<R, Errno>>
    where
        S: FnOnce(&mut Rc<Promissory<R, C>>) -> Poll<Result<R, Errno>>,
    {
        self.0.set_requesting();

        match (start_fn)(&mut self.0) {
            Poll::Pending => (),
            Poll::Ready(res) => {
                assert!(matches!(self.0.set_kept(res), PromiseState::Requesting))
            }
        }

        FuturePromise(self.0)
    }
}
