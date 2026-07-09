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

/// A trait defining the interface for an executor that can schedule and execute tasks.
pub(crate) trait Executor {
    /// Returns `true` if the current executor is the same as this executor.
    fn is_current(&self) -> bool;

    /// Schedules a task for execution on this executor.
    fn schedule<F>(&self, f: F)
    where
        F: FnOnce() + 'static;
}

pub(crate) trait ArcTaskBase: Send + 'static {
    /// Returns the executor on which this task should be executed.
    fn executor(&self) -> impl Executor + 'static;
}

/// A way of scheduling and executing an asynchronous task in the SPDK Event Framework.
pub(crate) trait ArcTask: ArcTaskBase {
    /// Schedules a task for execution on its target, consuming the task in the process.
    fn schedule(arc_self: Arc<Self>) {
        let executor = arc_self.executor();

        // If the current executor is the target of this task, attempt to run it synchronously.
        if executor.is_current() && Self::run(&arc_self) {
            return;
        }

        // The task could not be run synchronously, so enqueue it to run on the target executor at a
        // later time.
        executor.schedule(move || assert!(Self::run(&arc_self)));
    }

    /// Schedules a task for execution without consuming the task.
    ///
    /// # Panics
    ///
    /// This method panics if this task's target executor is not the current executor.
    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let executor = arc_self.executor();

        assert!(executor.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(arc_self) {
            // The task could not be run synchronously, so enqueue it to run on the target executor
            // at a later time.
            let cloned_task = arc_self.clone();

            executor.schedule(move || assert!(Self::run(&cloned_task)));
        }
    }

    /// Executes a task on the executor.
    ///
    /// # Returns
    ///
    /// This method returns `true` if the task was run synchronously. If it returns `false`, the
    /// task could not be executed synchronously and should be scheduled to run later.
    fn run(arc_self: &Arc<Self>) -> bool;
}

/// Clones the [`RawWaker`] for this task.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_clone<W: ArcTask>(data: *const ()) -> RawWaker {
    Arc::<W>::increment_strong_count(data.cast());

    RawWaker::new(data, waker_vtable::<W>())
}

/// Wakes the task referenced by the given [`RawWaker`] consuming the `RawWaker` in the process.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_wake<W: ArcTask>(data: *const ()) {
    let arc_task = Arc::<W>::from_raw(data.cast());

    ArcTask::schedule(arc_task);
}

/// Wakes the task referenced by the given [`RawWaker`] without consuming the `RawWaker`.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_wake_by_ref<W: ArcTask>(data: *const ()) {
    let arc_task = ManuallyDrop::new(Arc::<W>::from_raw(data.cast()));

    ArcTask::schedule_by_ref(&arc_task);
}

/// Drops the given [`RawWaker`] releasing its resources.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_drop<W: ArcTask>(data: *const ()) {
    drop(Arc::<W>::from_raw(data.cast()));
}

/// Gets the [`RawWakerVTable`] used by a [`RawWaker`] to awaken a task.
fn waker_vtable<W: ArcTask>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        waker_clone::<W>,
        waker_wake::<W>,
        waker_wake_by_ref::<W>,
        waker_drop::<W>,
    )
}

/// Creates a reference to the [`Waker`] from a reference to a [`Task<T>`].
fn waker_ref<W: ArcTask>(arc_self: &Arc<W>) -> WakerRef<'_> {
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
    /// This method panics if the task is polled in a state other than `Pending` or `Polling`.
    fn poll(&mut self) -> Self {
        match self {
            Self::Pending => self.replace(Self::Polling),
            Self::Polling => Self::Polling,
            _ => panic!("task polled in unexpected state: {:?}", self),
        }
    }
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct Task<E, T, F>
where
    E: Executor + 'static,
    T: 'static,
    F: Future<Output = T> + 'static,
{
    executor: Option<E>,
    state: RefCell<TaskState>,
    result_sender: UnsafeCell<Option<oneshot::Sender<T>>>,
    future: UnsafeCell<F>,
}

unsafe impl<E, T, F> Send for Task<E, T, F>
where
    E: Executor + 'static,
    T: 'static,
    F: Future<Output = T> + 'static,
{
}

impl<T, F> Task<Thread, T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new task from a [`Future`] to be scheduled on a [`Thread`].
    ///
    /// If `executor` is `None`, the task will be scheduled to run on the application thread.
    ///
    /// # Return
    ///
    /// This function returns both a newly constructed [`ThreadTask`] and a [`JoinHandle`] that can
    /// be used to await the result of executing the [`Future`].
    pub(crate) fn with_future(executor: Option<Thread>, fut: F) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self {
            executor,
            state: RefCell::new(TaskState::Pending),
            result_sender: UnsafeCell::new(Some(sx)),
            future: UnsafeCell::new(fut),
        });

        (task, JoinHandle::new(rx))
    }
}

impl<T, F> ArcTaskBase for Task<Thread, T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(Thread::application)
    }
}

impl<T, F> Task<Reactor, T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new task from a [`Future`] to be scheduled on the given [`Reactor`]
    ///
    /// # Return
    ///
    /// This function returns both a newly constructed [`ReactorTask`] and a [`JoinHandle`] that can
    /// be used to await the result of executing the [`Future`].
    pub(crate) fn with_future(executor: Reactor, fut: F) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self {
            executor: Some(executor),
            state: RefCell::new(TaskState::Pending),
            result_sender: UnsafeCell::new(Some(sx)),
            future: UnsafeCell::new(fut),
        });

        (task, JoinHandle::new(rx))
    }
}

impl<T, F> ArcTaskBase for Task<Reactor, T, F>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor.unwrap()
    }
}

impl<E, T, F> ArcTask for Task<E, T, F>
where
    E: Executor + 'static,
    T: 'static,
    F: Future<Output = T> + 'static,
    Task<E, T, F>: ArcTaskBase,
{
    fn run(arc_self: &Arc<Self>) -> bool {
        let state = arc_self.state.borrow_mut().poll();

        match state {
            TaskState::Pending => {
                let waker = waker_ref(arc_self);
                let ctx = &mut Context::from_waker(&waker);

                // SAFETY: The future is only polled when in the `Polling` state (i.e. previous
                // state was `Pending`). There are no other references to it.
                let mut fut = unsafe { Pin::new_unchecked(&mut *arc_self.future.get()) };

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => arc_self.state.borrow_mut().mark_pending(),
                    Poll::Ready(r) => {
                        // SAFETY: The result sender is only taken when in the `Done` state. There
                        // are no other references to it.
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

/// Schedules a new asynchronous task to be executed on the given [`Thread`] and returns a
/// [`JoinHandle`] to await results.
///
/// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures that
/// may not be `Send` once started.
pub(crate) fn spawn_on_thread<G, F, T>(thread: Thread, fut_gen: G) -> JoinHandle<T>
where
    G: FnOnce() -> F + Send + 'static,
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
    let (task, join_handle) = Task::<Thread, T, F>::with_future(Some(thread), fut_gen());

    ArcTask::schedule(task);

    join_handle
}

/// Schedules a new asynchronous task to be executed on the current [`Thread`] and returns a
/// [`JoinHandle`] to await results.
pub(crate) fn spawn_on_current_thread<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let (task, join_handle) = Task::<Thread, T, F>::with_future(Thread::try_current(), fut);

    ArcTask::schedule(task);

    join_handle
}

/// Schedules a new asynchronous task to be executed on the given [`Reactor`] and returns a
/// [`JoinHandle`] to await results.
///
/// The indirection of `fut_gen` instead of receiving a `Future` directly allows for futures that
/// may not be `Send` once started.
pub(crate) fn spawn_on_reactor<G, F, T>(reactor: Reactor, fut_gen: G) -> JoinHandle<T>
where
    G: FnOnce() -> F + Send + 'static,
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
    let (task, join_handle) = Task::<Reactor, T, F>::with_future(reactor, fut_gen());

    ArcTask::schedule(task);

    join_handle
}

/// Schedules a new asynchronous task to be executed on the current [`Reactor`] and returns a
/// [`JoinHandle`] to await results.
pub(crate) fn spawn_on_current_reactor<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let (task, join_handle) = Task::<Reactor, T, F>::with_future(Reactor::current(), fut);

    ArcTask::schedule(task);

    join_handle
}
