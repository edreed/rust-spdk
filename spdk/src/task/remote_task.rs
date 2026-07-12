use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    future::Future,
    mem::{self, ManuallyDrop},
    pin::Pin,
    sync::{Arc, LazyLock},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use futures::task::WakerRef;
use parking_lot::{Mutex, RwLock};

use crate::{
    runtime::Reactor,
    task::{Executor, JoinHandle, RawJoinHandleVTable, ResultState, TaskBase},
    thread::Thread,
};

/// A way of scheduling and executing an asynchronous task on a different executor in the SPDK Event
/// Framework.
pub(crate) trait ArcTask: TaskBase {
    /// The type of value produced by the task when it completes.
    type Output: Send + 'static;

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
    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let executor = arc_self.executor();

        // If the current executor is the target of this task, attempt to run it synchronously.
        if executor.is_current() && Self::run(arc_self) {
            return;
        }

        // The task could not be run synchronously, so enqueue it to run on the target executor
        // at a later time.
        let cloned_task = arc_self.clone();

        executor.schedule(move || assert!(Self::run(&cloned_task)));
    }

    /// Executes a task on the executor.
    ///
    /// # Returns
    ///
    /// This method returns `true` if the task was run synchronously. If it returns `false`, the
    /// task could not be executed synchronously and should be scheduled to run later.
    fn run(arc_self: &Arc<Self>) -> bool;

    /// Polls the result of a task, returning `Poll::Pending` if the result is not yet ready, or
    /// `Poll::Ready` with the result if it is ready.
    fn poll_result(arc_self: &Arc<Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
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
const fn waker_vtable<W: ArcTask>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        waker_clone::<W>,
        waker_wake::<W>,
        waker_wake_by_ref::<W>,
        waker_drop::<W>,
    )
}

/// Creates a reference to the [`Waker`] from a reference to a [`RemoteTask`].
fn waker_ref<W: ArcTask>(arc_self: &Arc<W>) -> WakerRef<'_> {
    let data = Arc::as_ptr(arc_self).cast();

    let waker =
        ManuallyDrop::new(unsafe { Waker::from_raw(RawWaker::new(data, waker_vtable::<W>())) });

    WakerRef::new_unowned(waker)
}

/// Polls a task for its result without consuming the task.
///
/// # Returns
///
/// `Poll::Pending` if the task has not yet completed, or `Poll::Ready(result)` if the task has
/// completed and produced a result.
unsafe fn join_handle_poll_result<J: ArcTask>(
    data: *const (),
    cx: &mut Context<'_>,
) -> Poll<J::Output> {
    let arc_task = ManuallyDrop::new(Arc::<J>::from_raw(data.cast()));

    ArcTask::poll_result(&arc_task, cx)
}

/// Drops the task referenced by the given pointer, releasing its resources.
unsafe fn join_handle_drop<J: ArcTask>(data: *mut ()) {
    drop(Arc::<J>::from_raw(data.cast()));
}

static JOIN_HANDLE_VTABLE_MAP: LazyLock<RwLock<HashMap<TypeId, Box<RawJoinHandleVTable<()>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Gets the [`RawJoinHandleVTable`] used by a [`JoinHandle`] to poll a task.
pub(crate) fn join_handle_vtable<J: ArcTask>() -> &'static RawJoinHandleVTable<J::Output> {
    if let Some(vtable) = JOIN_HANDLE_VTABLE_MAP
        .read()
        .get(&TypeId::of::<J>())
        .map(|v| unsafe {
            &*(v as *const Box<RawJoinHandleVTable<()>>
                as *const Box<RawJoinHandleVTable<J::Output>>)
        })
    {
        return vtable.as_ref();
    }

    let vtable = unsafe {
        &*(JOIN_HANDLE_VTABLE_MAP
            .write()
            .entry(TypeId::of::<J>())
            .or_insert_with(|| {
                mem::transmute(Box::new(RawJoinHandleVTable::new(
                    join_handle_poll_result::<J>,
                    join_handle_drop::<J>,
                )))
            }) as *const Box<RawJoinHandleVTable<()>>
            as *const Box<RawJoinHandleVTable<J::Output>>)
    };

    vtable.as_ref()
}

/// Orchestrates the execution of a [`Future`] on another [`Reactor`] or [`Thread`].
pub(crate) struct RemoteTask<E, F, T>
where
    E: Executor + 'static,
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
    executor: Option<E>,
    result: Mutex<ResultState<T>>,
    future: RefCell<F>,
}

unsafe impl<E, F, T> Send for RemoteTask<E, F, T>
where
    E: Executor + 'static,
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
}

impl<F, T> RemoteTask<Thread, F, T>
where
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
    /// Constructs a new `RemoteTask` from a [`Future`] to be scheduled on the specified [`Thread`].
    pub(crate) fn with_future(executor: Option<Thread>, fut: F) -> Arc<Self> {
        Arc::new(Self {
            executor,
            result: Mutex::new(ResultState::Empty),
            future: RefCell::new(fut),
        })
    }
}

impl<F, T> TaskBase for RemoteTask<Thread, F, T>
where
    F: Future<Output = T> + 'static,
    T: Send + 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(Thread::application)
    }
}

impl<F, T> RemoteTask<Reactor, F, T>
where
    T: Send + 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new `RemoteTask` from a [`Future`] to be scheduled on the specified [`Reactor`].
    pub(crate) fn with_future(executor: Reactor, fut: F) -> Arc<Self> {
        Arc::new(Self {
            executor: Some(executor),
            result: Mutex::new(ResultState::Empty),
            future: RefCell::new(fut),
        })
    }
}

impl<F, T> TaskBase for RemoteTask<Reactor, F, T>
where
    T: Send + 'static,
    F: Future<Output = T> + 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor.unwrap()
    }
}

impl<E, F, T> ArcTask for RemoteTask<E, F, T>
where
    E: Executor + 'static,
    T: Send + 'static,
    F: Future<Output = T> + 'static,
    RemoteTask<E, F, T>: TaskBase,
{
    type Output = T;

    fn run(arc_self: &Arc<Self>) -> bool {
        match arc_self.future.try_borrow_mut() {
            Ok(mut fut) => {
                let waker = waker_ref(arc_self);
                let mut ctx = Context::from_waker(&waker);

                // SAFETY: The future is pinned in place for the duration of this function call, and
                // it is not moved or dropped until the future is complete. The future is also not
                // accessed from any other thread, so it is safe to create a pinned reference to it.
                let fut = unsafe { Pin::new_unchecked(&mut *fut) };

                match fut.poll(&mut ctx) {
                    Poll::Ready(result) => {
                        let res = arc_self.result.lock().set_result(result);

                        if let Some(waker) = res {
                            waker.wake();
                        };
                    }
                    Poll::Pending => {}
                }

                true
            }
            // The task was re-entered while its future was already being polled. Return `false` to
            // indicate that the task needs to be scheduled to run by its executor later.
            Err(_) => false,
        }
    }

    fn poll_result(arc_self: &Arc<Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        arc_self.result.lock().poll_result(cx)
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
    let task = RemoteTask::<Thread, F, T>::with_future(Some(thread), fut_gen());

    ArcTask::schedule_by_ref(&task);

    task.into()
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
    let task = RemoteTask::<Reactor, F, T>::with_future(reactor, fut_gen());

    ArcTask::schedule_by_ref(&task);

    task.into()
}
