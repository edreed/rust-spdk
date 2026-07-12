//! Asynchronous task management for the Storage Performance Development Kit
//! [Event Framework][SPEF].
//!
//! [SPEF]: https://spdk.io/doc/event.html
mod join_handle;
mod local_task;
mod poller;
mod promise;
mod remote_task;
mod yield_now;

use std::{
    mem,
    task::{Context, Poll, Waker},
};

pub(crate) use join_handle::RawJoinHandleVTable;
pub(crate) use local_task::{spawn_on_current_reactor, spawn_on_current_thread, LocalTask, RcTask};
pub(crate) use remote_task::{spawn_on_reactor, spawn_on_thread};

pub use join_handle::JoinHandle;
pub use poller::{polled_fn, polled_fn_with_period, Polled, PolledFn, Poller};
pub use promise::{Promise, Promissory};
pub use yield_now::yield_now;

/// Represents the state of a task's result.
#[derive(Default)]
enum ResultState<T> {
    /// The task has not yet completed and is waiting for the result to be produced.
    #[default]
    Empty,

    /// The task has not yet completed, but a [`Waker`] has been registered to be notified when the
    /// result is ready.
    Waiting(Waker),

    /// The task has completed and the result is ready to be consumed.
    Ready(T),

    /// The task has completed and the result has been consumed.
    Consumed,
}

impl<T> ResultState<T> {
    /// Sets the result of the task and wakes any registered [`Waker`].
    fn set_result(&mut self, result: T) -> Option<Waker> {
        match mem::replace(self, ResultState::Ready(result)) {
            ResultState::Empty => None,
            ResultState::Waiting(waker) => Some(waker),
            ResultState::Ready(_) => panic!("result already set"),
            ResultState::Consumed => panic!("result already consumed"),
        }
    }

    /// Polls the result of the task, returning `Poll::Pending` if the result is not yet ready, or
    /// `Poll::Ready` with the result if it is ready.
    fn poll_result(&mut self, cx: &mut Context<'_>) -> Poll<T> {
        match self {
            ResultState::Empty => {
                *self = ResultState::Waiting(cx.waker().clone());
                Poll::Pending
            }
            ResultState::Ready(_) => {
                let res = mem::replace(self, ResultState::Consumed);

                if let ResultState::Ready(res) = res {
                    Poll::Ready(res)
                } else {
                    unreachable!()
                }
            }
            ResultState::Waiting(_) => Poll::Pending,
            ResultState::Consumed => panic!("poll_result called after result was consumed"),
        }
    }
}

/// A trait defining the interface for an executor that can schedule and execute tasks.
pub(crate) trait Executor {
    /// Returns `true` if the current executor is the same as this executor.
    fn is_current(&self) -> bool;

    /// Schedules a task for execution on this executor.
    fn schedule<F>(&self, f: F)
    where
        F: FnOnce() + 'static;
}

/// A trait defining the base interface for a task.
pub(crate) trait TaskBase: Send + 'static {
    /// Returns the executor on which this task should be executed.
    fn executor(&self) -> impl Executor + 'static;
}
