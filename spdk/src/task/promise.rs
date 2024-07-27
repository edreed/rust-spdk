#![allow(dead_code)]
use std::{
    cell::UnsafeCell,
    fmt::Debug,
    mem,
    pin::Pin,
    sync::{
        Arc,
        Mutex,
    },
    task::{
        Context,
        Poll,
        Waker,
    }
};

use futures::Future;
use libc::c_void;
use ternary_rs::if_else;

use crate::{
    errors::Errno,
    thread::Thread,
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
enum PromiseState<T: Debug + Send + 'static> {
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

unsafe impl<T: Debug + Send + 'static> Send for PromiseState<T> {}

impl <T: Debug + Send + 'static> PromiseState<T> {
    /// Returns a new `PromiseState` instance in the [`Empty`] state.
    /// 
    /// [`Empty`]: type@PromiseState::Empty
    fn new() -> Arc<Mutex<Self>> {
        Default::default()
    }

    /// Replaces the current state with the specified value and returns the old state.
    fn replace(&mut self, value: Self) -> Self {
        mem::replace(&mut *self, value)
    }

    /// Sets the result of the operation and awakens the [`Promise`] awaiting the result.
    fn set_result(arc_self: &Mutex<Self>, res: Result<T, Errno>) {
        let prev_state = match arc_self.try_lock() {
            Ok(mut state) => {
                Ok(state.replace(Self::Kept(res)))
            },
            Err(_) => Err(res),
        };

        match prev_state {
            Ok(prev_state) => match prev_state {
                Self::Requesting => (),
                Self::Waiting(waker) => waker.wake(),
                _ => panic!("promise kept in unexpected state: {:?}", prev_state),
            }
            Err(res) => {
                Thread::current().send_msg(move || {
                    Self::set_result(arc_self, res);
                }).expect("send result");
            },
        }
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

#[derive(PartialEq, Copy, Clone, Debug)]
struct RawPromiseVtable<T>
where
    T: Debug + Send + 'static
{
    poll: unsafe fn (*const (), &mut Context<'_>) -> Poll<Result<T, Errno>>,
    set_result: unsafe fn (*const (), Result<T, Errno>),
    drop: unsafe fn (*const ()),
}

struct RawPromise<T>
where
    T: Debug + Send + 'static
{
    data: *const (),
    vtable: &'static RawPromiseVtable<T>,
}

impl <T> RawPromise<T>
where
    T: Debug + Send + 'static
{
    fn new(data: *const (), vtable: &'static RawPromiseVtable<T>) -> Self {
        Self { data, vtable }
    }
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback receives a raw pointer to an SPDK object of type `R` and
/// converts it to the appropriate Rust type `T`. If the received pointer is
/// null, the result is a suitable `Err(Errno)` value.
pub unsafe extern "C" fn complete_with_object<T, R>(cx: *mut c_void, obj: *mut R)
where
    T: Debug + Send + TryFrom<*mut R, Error = Errno> + 'static,
{
    Promise::<T>::set_result(cx.cast(), obj.try_into());
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback receives a status code. If the status code is 0, the result is
/// `Ok(())`. Otherwise, the status is converted to a suitable `Err(Errno)`
/// value.
pub unsafe extern "C" fn complete_with_status(cx: *mut c_void, status: i32) {
    let arc_self = Arc::from_raw(cx.cast::<Mutex<PromiseState<()>>>());
    let res = if_else!(status == 0, Ok(()), Err(Errno(-status)));

    PromiseState::set_result(arc_self, res);
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback always sets the result to `Ok(())`.
pub unsafe extern "C" fn complete_with_ok(cx: *mut c_void) {
    let arc_self = Arc::from_raw(cx.cast::<Mutex<PromiseState<()>>>());

    PromiseState::set_result(arc_self, Ok(()));
}

struct Promissory<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    state: Mutex<PromiseState<T>>,
    start_fn: UnsafeCell<F>,
}

impl <F, T> Promissory<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    fn new(start_fn: F) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PromiseState::Empty),
            start_fn: UnsafeCell::new(start_fn),
        })
    }

    fn poll(
        arc_self: Arc<Self>,
        cx: &mut Context<'_>
    ) -> Poll<Result<T, Errno>> {
        let state = arc_self.state.lock().unwrap().poll(cx);

        match state {
            PromiseState::Empty =>  {
                let promise_cx = Arc::into_raw(arc_self.clone());
                let start_fn = unsafe { &mut *arc_self.start_fn.get() };

                match start_fn(promise_cx as *mut c_void) {
                    Poll::Pending => {
                        let state = arc_self.state.lock().unwrap().poll(cx);

                        if let PromiseState::Kept(res) = state {
                            return Poll::Ready(res);
                        }

                        Poll::Pending
                    },
                    Poll::Ready(res) => {
                        unsafe { Arc::from_raw(promise_cx) };

                        Poll::Ready(res)
                    }
                }
            },
            PromiseState::Waiting(_) => Poll::Pending,
            PromiseState::Kept(res) => Poll::Ready(res),
            _ => unreachable!("promise already fulfilled"),
        }
    }
}

unsafe fn promissory_poll<F, T>(data: *const (), cx: &mut Context<'_>) -> Poll<Result<T, Errno>>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    Promissory::poll(Arc::<Promissory<F, T>>::from_raw(data.cast()), cx)
}

unsafe fn promissory_set_result<F, T>(data: *const (), res: Result<T, Errno>)
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    let arc_self = Arc::<Promissory<F, T>>::from_raw(data.cast());

    PromiseState::<T>::set_result(&arc_self.state, res);
}

unsafe fn promissory_drop<F, T>(data: *const ())
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    Arc::<Promissory<F, T>>::from_raw(data.cast());
}

const fn promissory_vtable<F, T>() -> &'static RawPromiseVtable<T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: Debug + Send + 'static
{
    &RawPromiseVtable::<T> {
        poll: promissory_poll::<F, T>,
        set_result: promissory_set_result::<F, T>,
        drop: promissory_drop::<F, T>,
    }
}

/// Orchestrates the execution of an asynchronous operation and provides access
/// to its result.
pub struct Promise<T>
where
    T: Debug + Send + 'static
{
    promise: RawPromise<T>,
}

unsafe impl<T> Send for Promise<T>
where
    T: Debug + Send + 'static
{}

impl<T> Promise<T>
where
    T: Debug + Send + 'static
{
    /// Returns a new `Promise` instance.
    /// 
    /// The caller provides a function that starts the asynchronous operation
    /// passing one of the `complete_with_*` callbacks and the provided context
    /// pointer to the SPDK API.
    pub fn new<F>(start_fn: F) -> Self
    where
        F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>
    {
        let promissory = Promissory::new(start_fn);

        Self {
            promise: RawPromise::new(
                Arc::into_raw(promissory).cast(),
                promissory_vtable::<F, T>(),
            ),
        }
    }

    fn set_result(&self, res: Result<T, Errno>) {
        let set_result = self.promise.vtable.set_result;
        let data = self.promise.data;

        unsafe { set_result(data, res) }
    }
}

impl<T> Future for Promise<T>
where
    T: Debug + Send + 'static
{
    type Output = Result<T, Errno>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Self::Output> {
        let poll = self.promise.vtable.poll;
        let data = self.promise.data;

        unsafe { poll(data, cx) }
    }
}
