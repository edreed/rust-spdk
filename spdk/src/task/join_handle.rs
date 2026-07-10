use std::{
    future::Future,
    task::{Context, Poll},
};

/// A virtual function table (vtable) that specifies the operations that can be performed on a
/// [`RawJoinHandle`].
///
/// The pointer passed to all functions in this vtable is the `data` pointer of the enclosing
/// [`RawJoinHandle`] object. The vtable is used to construct a [`RawJoinHandle`] that is embedded
/// in a [`JoinHandle`]. The vtable is used by `JoinHandle` orchestrate receiving the result of an
/// asynchronous operation.
pub(crate) struct RawJoinHandleVTable<T>
where
    T: 'static,
{
    poll_result: unsafe fn(*const (), &mut Context<'_>) -> Poll<T>,
    drop: unsafe fn(*mut ()),
}

impl<T> RawJoinHandleVTable<T>
where
    T: 'static,
{
    /// Creates a new `RawJoinHandleVTable` with the specified `poll_result` and `drop` functions.
    ///
    /// `poll_result`
    ///
    /// This function is called when a [`JoinHandle`] is polled thorugh its [`Future`] trait
    /// implementation. It returns a [`Poll<T>`] value indicating whether the task has completed
    /// and, if so, the result of the task.
    ///
    /// `drop
    ///
    /// This function is called when a [`JoinHandle`] is dropped. It should perform any necessary
    /// cleanup for the task.
    pub(crate) const fn new(
        poll_result: unsafe fn(*const (), &mut Context<'_>) -> Poll<T>,
        drop: unsafe fn(*mut ()),
    ) -> Self {
        Self { poll_result, drop }
    }
}

/// A raw handle to a task that can be used to await the result of the task.
pub(crate) struct RawJoinHandle<T>
where
    T: 'static,
{
    vtable: &'static RawJoinHandleVTable<T>,
    data: *mut (),
}

/// A handle that awaits the result of a task.
///
/// Dropping a [`JoinHandle`] will detach the task leaving no way to join on
/// it or obtain its result.
///
/// A [`JoinHandle`] is created when a task is spawned.
pub struct JoinHandle<T>
where
    T: 'static,
{
    raw: RawJoinHandle<T>,
}

impl<T> JoinHandle<T>
where
    T: 'static,
{
    /// Creates a new `JoinHandle` with the specified `data` pointer and `vtable`.
    pub(crate) const unsafe fn new(data: *mut (), vtable: &'static RawJoinHandleVTable<T>) -> Self {
        Self {
            raw: RawJoinHandle { vtable, data },
        }
    }
}

impl<T> Future for JoinHandle<T>
where
    T: 'static,
{
    type Output = T;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { (self.raw.vtable.poll_result)(self.raw.data, cx) }
    }
}

impl<T> Drop for JoinHandle<T>
where
    T: 'static,
{
    fn drop(&mut self) {
        unsafe { (self.raw.vtable.drop)(self.raw.data) }
    }
}
