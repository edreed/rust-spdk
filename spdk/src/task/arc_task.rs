//! Asynchronous task management for Storage Performance Development Kit [Event
//! Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
use std::{
    future::Future,
    sync::{
        Arc,
        Mutex,
    },
    task::{
        Context,
        Poll,
    }
};

use futures::{
    channel::oneshot,
    future::BoxFuture,
    FutureExt,
    task::{
        ArcWake,

        waker_ref
    },
};

use crate::thread::Thread;

use super::JoinHandle;

/// A way of scheduling and executing a [`Task`] on a [`Thread`].
pub(crate) trait ArcTask: Send + Sync {
    /// Schedules a task on for execution on its target thread.
    fn schedule(arc_self: &Arc<Self>);

    /// Executes a task on the current thread.
    /// 
    /// # Returns
    /// 
    /// This method returns `true` if the task was run synchronously. If it
    /// returns `false`, the task could not be executed synchronously and
    /// should be scheduled to run later.
    /// 
    /// # Panics
    /// 
    /// This method panics if this task's target thread is not the current
    /// thread.
    fn run(arc_self: &Arc<Self>) -> bool;
} 

/// Encapsulates the execution state of a [`Future`].
pub(crate) struct TaskState<T: Send + 'static> {
    future: Option<BoxFuture<'static, T>>,
    result_sender: Option<oneshot::Sender<T>>
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct Task<T: Send + 'static> {
    target_thread: Thread,
    state: Mutex<TaskState<T>>,
}

impl <T: Send + 'static> Task<T> {
    /// Constructs a new task from a [`Future`].
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`Task`] and a
    /// [`JoinHandle`] that can be used to await the result of
    /// executing the [`Future`].
    pub(crate) fn with_future(
        target_thread: &Thread,
        fut: impl Future<Output = T> + Send + 'static
    ) -> (Arc<Task<T>>, JoinHandle<T>) {
        Self::with_boxed(target_thread, fut.boxed())
    }

    /// Constructs a new task from a [`BoxFuture`].
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`Task`] and a
    /// [`JoinHandle`] that can be used to await the result of
    /// executing the [`Future`].
    pub(crate) fn with_boxed(
        target_thread: &Thread,
        boxed_fut: BoxFuture<'static, T>
    ) -> (Arc<Task<T>>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Task{
            target_thread: *target_thread,
            state: Mutex::new(TaskState {
                future: Some(boxed_fut),
                result_sender: Some(sx)
            })
        });

        (task, JoinHandle::new(rx))
    }
}

impl <T: Send + 'static> ArcTask for Task<T> {
    fn schedule(arc_self: &Arc<Self>) {
        // If the current thread is the target of this task, attempt to run it
        // synchronously.
        if let Some(t) = Thread::try_current() {
            if t == arc_self.target_thread {
                if Self::run(arc_self) {
                    return;
                }
            }
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // target thread.
        let cloned_task = arc_self.clone();

        arc_self.target_thread.send_msg(move || {
            ArcTask::run(&cloned_task);
        }).unwrap();
    }

    fn run(arc_self: &Arc<Self>) -> bool {
        assert!(arc_self.target_thread.is_current());

        let lock = arc_self.state.try_lock();

        if let Ok(mut task_state) = lock {
            if let Some(mut fut) = task_state.future.take() {
                let waker = waker_ref(&arc_self);
                let ctx = &mut Context::from_waker(&waker);

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => task_state.future = Some(fut),
                    Poll::Ready(r) => if let Some(s) = task_state.result_sender.take() {
                        _ = s.send(r);
                    },
                }
            }
            return true;
        }

        false
    }
}

impl <T: Send + 'static> ArcWake for Task<T> {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        ArcTask::schedule(arc_self);
    }
}
