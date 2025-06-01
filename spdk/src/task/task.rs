use std::{
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    future::Future,
    mem::{self, ManuallyDrop},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use futures::{channel::oneshot, task::WakerRef};

use crate::{runtime::Reactor, thread::Thread};

use super::JoinHandle;

/// A way of scheduling and executing an asynchronous task in the SPDK Event
/// Framework.
pub(crate) trait Task: Send {
    /// Schedules a task for execution on its target, consuming the task in the
    /// process.
    fn schedule(arc_self: Arc<Self>);

    /// Schedules a task for execution without consuming the task.
    ///
    /// # Panics
    ///
    /// This method panics if this task's target executor is not the current
    /// executor.
    fn schedule_by_ref(arc_self: &Arc<Self>);
}

/// Clones the [`RawWaker`] for this task.
///
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_clone<W: Task>(data: *const ()) -> RawWaker {
    Arc::<W>::increment_strong_count(data.cast());

    RawWaker::new(data, waker_vtable::<W>())
}

/// Wakes the task referenced by the given [`RawWaker`] consuming the `RawWaker`
/// in the process.
///
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_wake<W: Task>(data: *const ()) {
    let rc_task = Arc::<W>::from_raw(data.cast());

    Task::schedule(rc_task);
}

/// Wakes the task referenced by the given [`RawWaker`] without consuming the
/// `RawWaker`.
///
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_wake_by_ref<W: Task>(data: *const ()) {
    let rc_task = ManuallyDrop::new(Arc::<W>::from_raw(data.cast()));

    Task::schedule_by_ref(&rc_task);
}

/// Drops the given [`RawWaker`] releasing its resources.
///
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_drop<W: Task>(data: *const ()) {
    drop(Arc::<W>::from_raw(data.cast()));
}

/// Gets the [`RawWakerVTable`] used by a [`RawWaker`] to awaken a task.
fn waker_vtable<W: Task>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        waker_clone::<W>,
        waker_wake::<W>,
        waker_wake_by_ref::<W>,
        waker_drop::<W>,
    )
}

/// Creates a reference to the [`Waker`] from a reference to a [`Task<T>`].
fn waker_ref<W: Task>(arc_self: &Arc<W>) -> WakerRef<'_> {
    let data = Arc::as_ptr(arc_self).cast();

    let waker =
        ManuallyDrop::new(unsafe { Waker::from_raw(RawWaker::new(data, waker_vtable::<W>())) });

    WakerRef::new_unowned(waker)
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Encapsulates the execution state of a [`Task`].
///
/// The following state diagram shows the state transitions of a [`Task`].
///
/// ```mermaid
/// stateDiagram-v2
/// Pending --> Polling : poll()
/// Polling --> Polling : poll()
/// Polling --> Pending : mark_pending()
/// Polling --> Done : mark_done()
/// ```
#[derive(Debug, Eq, PartialEq)]
enum TaskState {
    /// The task is pending execution.
    Pending,

    /// The task is currently being polled.
    Polling,

    /// The task has completed execution.
    Done,
}

impl TaskState {
    /// Replaces the current state with the specified value and returns the old state.
    #[inline]
    fn replace(&mut self, value: Self) -> Self {
        mem::replace(&mut *self, value)
    }

    /// Marks the task state as pending.
    ///
    /// # Panics
    ///
    /// This method panics if the current state is not `Polling`.
    #[inline]
    fn mark_pending(&mut self) {
        assert_eq!(self.replace(Self::Pending), Self::Polling);
    }

    /// Marks the task state as done.
    ///
    /// # Panics
    ///
    /// This method panics if the current state is not `Polling`.
    #[inline]
    fn mark_done(&mut self) {
        assert_eq!(self.replace(Self::Done), Self::Polling);
    }

    /// Polls the state of the task, advancing to the next state if possible.
    ///
    /// # Panics
    ///
    /// This method panics if the task is polled in a state other than `Pending`
    /// or `Polling`.
    fn poll(&mut self) -> Self {
        match self {
            Self::Pending => self.replace(Self::Polling),
            Self::Polling => Self::Polling,
            _ => panic!("task polled in unexpected state: {:?}", self),
        }
    }
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct ThreadTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    target_thread: Option<Thread>,
    state: RefCell<TaskState>,
    result_sender: UnsafeCell<Option<oneshot::Sender<T>>>,
    future: UnsafeCell<F>,
}

unsafe impl<T, F> Send for ThreadTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
}

impl<T, F> ThreadTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new task from a [`Future`] to be scheduled on a [`Thread`].
    ///
    /// If `target_thread` is `None`, the task will be scheduled to run on the
    /// application thread.
    ///
    /// # Return
    ///
    /// This function returns both a newly constructed [`ThreadTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_future(target_thread: Option<Thread>, fut: F) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self {
            target_thread,
            state: RefCell::new(TaskState::Pending),
            result_sender: UnsafeCell::new(Some(sx)),
            future: UnsafeCell::new(fut),
        });

        (task, JoinHandle::new(rx))
    }

    /// Returns the target [`Thread`] of this task.
    fn target_thread(&self) -> Thread {
        self.target_thread
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(Thread::application)
    }

    /// Executes a task on the executor.
    ///
    /// # Returns
    ///
    /// This method returns `true` if the task was run synchronously. If it
    /// returns `false`, the task could not be executed synchronously and
    /// should be scheduled to run later.
    fn run(arc_self: &Arc<Self>) -> bool {
        let state = arc_self.state.borrow_mut().poll();

        match state {
            TaskState::Pending => {
                let waker = waker_ref(arc_self);
                let ctx = &mut Context::from_waker(&waker);

                // SAFETY: The future is only polled when in the `Polling` state
                // (i.e. previous state was `Pending`). There are no other
                // references to it.
                let mut fut = unsafe { Pin::new_unchecked(&mut *arc_self.future.get()) };

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => arc_self.state.borrow_mut().mark_pending(),
                    Poll::Ready(r) => {
                        // SAFETY: The result sender is only taken when in the
                        // `Done` state. There are no other references to it.
                        let _ = unsafe { &mut *arc_self.result_sender.get() }
                            .take()
                            .unwrap()
                            .send(r);
                        arc_self.state.borrow_mut().mark_done();
                    }
                }

                true
            }
            TaskState::Polling => false,
            _ => unreachable!(),
        }
    }
}

impl<T, F> Task for ThreadTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    fn schedule(arc_self: Arc<Self>) {
        let target_thread = arc_self.target_thread();

        // If the current thread is the target of this task, attempt to run it
        // synchronously.
        if target_thread.is_current() && Self::run(&arc_self) {
            return;
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this thread at a later time.
        target_thread
            .send_msg(move || assert!(Self::run(&arc_self)))
            .unwrap();
    }

    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let target_thread = arc_self.target_thread();

        assert!(target_thread.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(arc_self) {
            // The task could not be run synchronously, so enqueue it to run on the
            // this thread at a later time.
            let cloned_task = arc_self.clone();

            target_thread
                .send_msg(move || assert!(Self::run(&cloned_task)))
                .unwrap();
        }
    }
}

/// Orchestrates the execution of a [`Future`] on a reactor.
pub(crate) struct ReactorTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    target_reactor: Reactor,
    state: RefCell<TaskState>,
    result_sender: UnsafeCell<Option<oneshot::Sender<T>>>,
    future: UnsafeCell<F>,
}

unsafe impl<T, F> Send for ReactorTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
}

impl<T, F> ReactorTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new task from a [`Future`] to be scheduled on the given
    /// [`Reactor`]
    ///
    /// # Return
    ///
    /// This function returns both a newly constructed [`ReactorTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_future(target_reactor: Reactor, fut: F) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self {
            target_reactor,
            state: RefCell::new(TaskState::Pending),
            result_sender: UnsafeCell::new(Some(sx)),
            future: UnsafeCell::new(fut),
        });

        (task, JoinHandle::new(rx))
    }

    /// Executes a task on the executor.
    ///
    /// # Returns
    ///
    /// This method returns `true` if the task was run synchronously. If it
    /// returns `false`, the task could not be executed synchronously and
    /// should be scheduled to run later.
    fn run(arc_self: &Arc<Self>) -> bool {
        let state = arc_self.state.borrow_mut().poll();

        match state {
            TaskState::Pending => {
                let waker = waker_ref(arc_self);
                let ctx = &mut Context::from_waker(&waker);

                // SAFETY: The future is only polled when in the `Polling` state
                // (i.e. previous state was `Pending`). There are no other
                // references to it.
                let mut fut = unsafe { Pin::new_unchecked(&mut *arc_self.future.get()) };

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => arc_self.state.borrow_mut().mark_pending(),
                    Poll::Ready(r) => {
                        // SAFETY: The result sender is only taken when in the
                        // `Done` state. There are no other references to it.
                        let _ = unsafe { &mut *arc_self.result_sender.get() }
                            .take()
                            .unwrap()
                            .send(r);
                        arc_self.state.borrow_mut().mark_done();
                    }
                }

                true
            }
            TaskState::Polling => false,
            _ => unreachable!(),
        }
    }
}

impl<T, F> Task for ReactorTask<T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    fn schedule(arc_self: Arc<Self>) {
        let target_reactor = arc_self.target_reactor;

        // If the current reactor is the target of this task, attempt to run it
        // synchronously.
        if target_reactor.is_current() && Self::run(&arc_self) {
            return;
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this reactor at a later time.
        target_reactor
            .send_event(move || assert!(Self::run(&arc_self)))
            .unwrap();
    }

    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let target_reactor = arc_self.target_reactor;

        assert!(target_reactor.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(arc_self) {
            // The task could not be run synchronously, so enqueue it to run on
            // the this reactor at a later time.
            let cloned_task = arc_self.clone();

            target_reactor
                .send_event(move || assert!(Self::run(&cloned_task)))
                .unwrap();
        }
    }
}
