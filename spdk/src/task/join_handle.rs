use std::{
    future::Future,
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
};

use crate::task::{
    RcTask, local_task,
    remote_task::{self, ArcTask},
};

/// A virtual function table (vtable) that specifies the operations that can be performed on a
/// [`RawJoinHandle`].
///
/// The pointer passed to all functions in this vtable is the `data` pointer of the enclosing
/// [`RawJoinHandle`] object. The vtable is used to construct a [`RawJoinHandle`] that is embedded
/// in a [`JoinHandle`]. The vtable is used by `JoinHandle` to orchestrate receiving the result of
/// an asynchronous operation.
pub(crate) struct RawJoinHandleVTable<R>
where
    R: 'static,
{
    /// This function is called when a [`JoinHandle`] is polled through its [`Future`] trait
    /// implementation. It returns a [`Poll<T>`] value indicating whether the task has completed
    /// and, if so, the result of the task.
    pub(crate) poll_result: unsafe fn(*const (), &mut Context<'_>) -> Poll<R>,

    /// This function is called when a [`JoinHandle`] is dropped. It should perform any necessary
    /// cleanup for the task.
    pub(crate) drop: unsafe fn(*mut ()),
}

/// A raw handle to a task that can be used to await the result of the task.
pub(crate) struct RawJoinHandle<R>
where
    R: 'static,
{
    vtable: &'static RawJoinHandleVTable<R>,
    data: *mut (),
}

/// A handle that awaits the result of a task.
///
/// Dropping a [`JoinHandle`] will detach the task leaving no way to join on
/// it or obtain its result.
///
/// A [`JoinHandle`] is created when a task is spawned.
pub struct JoinHandle<'a, F, R>
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    raw: RawJoinHandle<R>,
    _task: PhantomData<&'a F>,
}

impl<'a, F, R> JoinHandle<'a, F, R>
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    /// Creates a new `JoinHandle` with the specified `data` pointer and `vtable`.
    const unsafe fn new(data: *mut (), vtable: &'static RawJoinHandleVTable<R>) -> Self {
        Self {
            raw: RawJoinHandle { vtable, data },
            _task: PhantomData,
        }
    }

    /// Creates a `JoinHandle` from a local task reference counted by `Rc`.
    pub(crate) fn from_local_task<T>(rc: Rc<T>) -> Self
    where
        T: RcTask<Output = R>,
    {
        let vtable = local_task::join_handle_vtable::<T>();
        let data = Rc::into_raw(rc).cast_mut() as *mut _;

        unsafe { Self::new(data, vtable) }
    }

    /// Creates a `JoinHandle` from a remote task reference counted by `Arc`.
    pub(crate) fn from_remote_task<T>(arc: Arc<T>) -> Self
    where
        T: ArcTask<Output = R>,
    {
        let vtable = remote_task::join_handle_vtable::<T>();
        let data = Arc::into_raw(arc).cast_mut() as *mut _;

        unsafe { Self::new(data, vtable) }
    }
}

impl<'a, F, R> Future for JoinHandle<'a, F, R>
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    type Output = R;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { (self.raw.vtable.poll_result)(self.raw.data, cx) }
    }
}

impl<'a, F, R> Drop for JoinHandle<'a, F, R>
where
    F: Future<Output = R> + 'a,
    R: 'static,
{
    fn drop(&mut self) {
        unsafe { (self.raw.vtable.drop)(self.raw.data) }
    }
}
