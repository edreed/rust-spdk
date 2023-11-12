#![allow(dead_code)]
use std::{
    cell::RefCell,
    rc::Rc,
    task::{
        Context,
        Poll,
        Waker,
    },
    pin::Pin,
};

use futures::Future;
use libc::c_void;
use ternary_rs::if_else;

use crate::{
    errors::Errno,
    thread::Thread,
};

/// Encapsulates the state of a [`Promise`].
struct PromiseState<T: 'static> {
    result: Option<Result<T, Errno>>,
    waker: Option<Waker>,
}

unsafe impl<T: 'static> Send for PromiseState<T> {}

impl <T: 'static> PromiseState<T> {
    /// Returns a new `PromiseState` instance.
    fn new() -> Rc<RefCell<Self>> {
        Default::default()
    }

    /// Sets the result of the operation and awakens the [`Promise`] awaiting the result.
    fn set_result(rc_self: &Rc<RefCell<Self>>, res: Result<T, Errno>) {
        let waker = match rc_self.try_borrow_mut() {
            Ok(mut state) => {
                state.result = Some(res);
                state.waker.take()
            },
            Err(_) => {
                let state = rc_self.clone();

                Thread::current().send_msg(move || {
                    let waker = {
                        let mut state = state.borrow_mut();

                        state.result = Some(res);
                        state.waker.take()
                    };

                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }).expect("send result");

                None
            },
        };

        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl <T: 'static> Default for PromiseState<T> {
    fn default() -> Self {
        Self {
            result: None,
            waker: None,
        }
    }
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback receives a raw pointer to an SPDK object of type `R` and
/// converts it to the appropriate Rust type `T`. If the received pointer is
/// null, the result is a suitable `Err(Errno)` value.
pub(crate) unsafe extern "C" fn complete_with_object<T, R>(cx: *mut c_void, obj: *mut R)
where
    T: TryFrom<*mut R, Error = Errno> + 'static,
{
    let rc_self = Rc::from_raw(cx.cast::<RefCell<PromiseState<T>>>());

    PromiseState::<T>::set_result(&rc_self, obj.try_into());
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback receives a status code. If the status code is 0, the result is
/// `Ok(())`. Otherwise, the status is converted to a suitable `Err(Errno)`
/// value.
pub(crate) unsafe extern "C" fn complete_with_status(cx: *mut c_void, status: i32) {
    let rc_self = Rc::from_raw(cx.cast::<RefCell<PromiseState<()>>>());
    let res = if_else!(status == 0, Ok(()), Err(Errno(-status)));

    PromiseState::set_result(&rc_self, res);
}

/// A callback invoked to set the result of a [`Promise`].
/// 
/// This callback always sets the result to `Ok(())`.
pub(crate) unsafe extern "C" fn complete_with_ok(cx: *mut c_void) {
    let rc_self = Rc::from_raw(cx.cast::<RefCell<PromiseState<()>>>());

    PromiseState::set_result(&rc_self, Ok(()));
}

/// Orchestrates the execution of an asynchronous operation and provides access
/// to its result.
pub(crate) struct Promise<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: 'static
{
    start_fn: F,
    state: Rc<RefCell<PromiseState<T>>>,
}

unsafe impl<F, T> Send for Promise<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: 'static
{}

impl<F, T> Promise<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>>,
    T: 'static
{
    /// Returns a new `Promise` instance.
    /// 
    /// The caller provides a function that starts the asynchronous operation
    /// passing one of the `complete_with_*` callbacks and the provided context
    /// pointer to the SPDK API.
    pub(crate) fn new(start_fn: F) -> Self {
        Self {
            start_fn,
            state: PromiseState::new(),
        }
    }

    /// Polls for the result of the operation setting the waker if the result is
    /// not available yet.
    fn poll_result(&mut self, cx: &Context<'_>) -> Poll<Result<T, Errno>> {
        let mut state = self.state.borrow_mut();

        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            },
        }
    }
}

impl<F, T> Future for Promise<F, T>
where
    F: FnMut(*mut c_void) -> Poll<Result<T, Errno>> + Unpin,
    T: 'static
{
    type Output = Result<T, Errno>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Self::Output> {
        match self.poll_result(cx) {
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => {
                let promise_cx = Rc::into_raw(self.state.clone());

                match (self.as_mut().start_fn)(promise_cx as *mut c_void) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(res) => {
                        unsafe { Rc::from_raw(promise_cx) };

                        Poll::Ready(res)
                    },
                }

            },
        }
    }
}
