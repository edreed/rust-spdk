use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    future::Future,
    mem::{self, ManuallyDrop},
    pin::Pin,
    rc::Rc,
    sync::LazyLock,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use futures::task::WakerRef;
use parking_lot::RwLock;

use crate::{
    runtime::Reactor,
    task::{Executor, JoinHandle, RawJoinHandleVTable, ResultState, TaskBase},
    thread::Thread,
};

/// A way of scheduling and executing an asynchronous task on the current executor in the SPDK Event
/// Framework.
pub(crate) trait RcTask: TaskBase {
    type Output: 'static;

    /// Schedules a task for execution on its target, consuming the task in the process.
    fn schedule(rc_self: Rc<Self>) {
        let executor = rc_self.executor();

        // If the current executor is the target of this task, attempt to run it synchronously.
        if executor.is_current() && Self::run(&rc_self) {
            return;
        }

        // The task could not be run synchronously, so enqueue it to run on the target executor at a
        // later time.
        executor.schedule(move || assert!(Self::run(&rc_self)));
    }

    /// Schedules a task for execution without consuming the task.
    ///
    /// # Panics
    ///
    /// This method panics if this task's target executor is not the current executor.
    fn schedule_by_ref(rc_self: &Rc<Self>) {
        let executor = rc_self.executor();

        assert!(executor.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(rc_self) {
            // The task could not be run synchronously, so enqueue it to run on the target executor
            // at a later time.
            let cloned_task = rc_self.clone();

            executor.schedule(move || assert!(Self::run(&cloned_task)));
        }
    }

    /// Executes a task on the executor.
    ///
    /// # Returns
    ///
    /// This method returns `true` if the task was run synchronously. If it returns `false`, the
    /// task could not be executed synchronously and should be scheduled to run later.
    fn run(rc_self: &Rc<Self>) -> bool;

    /// Polls the result of a task, returning `Poll::Pending` if the result is not yet ready, or
    /// `Poll::Ready` with the result if it is ready.
    fn poll_result(rc_self: &Rc<Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

/// Clones the [`RawWaker`] for this task.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_clone<W: RcTask>(data: *const ()) -> RawWaker {
    Rc::<W>::increment_strong_count(data.cast());

    RawWaker::new(data, waker_vtable::<W>())
}

/// Wakes the task referenced by the given [`RawWaker`] consuming the `RawWaker` in the process.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_wake<W: RcTask>(data: *const ()) {
    let rc_task = Rc::<W>::from_raw(data.cast());

    RcTask::schedule(rc_task);
}

/// Wakes the task referenced by the given [`RawWaker`] without consuming the `RawWaker`.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_wake_by_ref<W: RcTask>(data: *const ()) {
    let rc_task = ManuallyDrop::new(Rc::<W>::from_raw(data.cast()));

    RcTask::schedule_by_ref(&rc_task);
}

/// Drops the given [`RawWaker`] releasing its resources.
///
/// This function is invoked through the [`RawWakerVTable`] created by the [`waker_ref<W>`]
/// function.
unsafe fn waker_drop<W: RcTask>(data: *const ()) {
    drop(Rc::<W>::from_raw(data.cast()));
}

/// Gets the [`RawWakerVTable`] used by a [`RawWaker`] to awaken a task.
const fn waker_vtable<W: RcTask>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        waker_clone::<W>,
        waker_wake::<W>,
        waker_wake_by_ref::<W>,
        waker_drop::<W>,
    )
}

/// Creates a reference to the [`Waker`] from a reference to a [`LocalTask`].
fn waker_ref<W: RcTask>(rc_self: &Rc<W>) -> WakerRef<'_> {
    let data = Rc::as_ptr(rc_self).cast();

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
unsafe fn join_handle_poll_result<J: RcTask>(
    data: *const (),
    cx: &mut Context<'_>,
) -> Poll<J::Output> {
    let rc_task = ManuallyDrop::new(Rc::<J>::from_raw(data.cast()));

    RcTask::poll_result(&rc_task, cx)
}

/// Drops the task referenced by the given pointer, releasing its resources.
unsafe fn join_handle_drop<J: RcTask>(data: *mut ()) {
    drop(Rc::<J>::from_raw(data.cast()));
}

static JOIN_HANDLE_VTABLE_MAP: LazyLock<RwLock<HashMap<TypeId, Box<RawJoinHandleVTable<()>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Gets the [`RawJoinHandleVTable`] used by a [`JoinHandle`] to poll a task.
pub(crate) fn join_handle_vtable<J: RcTask>() -> &'static RawJoinHandleVTable<J::Output> {
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

/// Orchestrates the execution of a [`Future`] on the current [`Reactor`] or [`Thread`].
pub(crate) struct LocalTask<E, F, T>
where
    E: Executor + 'static,
    F: Future<Output = T> + 'static,
    T: 'static,
{
    executor: Option<E>,
    result: RefCell<ResultState<T>>,
    future: RefCell<F>,
}

unsafe impl<E, F, T> Send for LocalTask<E, F, T>
where
    E: Executor + 'static,
    F: Future<Output = T> + 'static,
    T: 'static,
{
}

impl<F, T> LocalTask<Thread, F, T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    /// Constructs a new `LocalTask` from a [`Future`] to be scheduled on the current [`Thread`].
    pub(crate) fn with_future(fut: F) -> Rc<Self> {
        Rc::new(Self {
            executor: Thread::try_current(),
            result: RefCell::new(ResultState::Empty),
            future: RefCell::new(fut),
        })
    }
}

impl<F, T> TaskBase for LocalTask<Thread, F, T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(Thread::application)
    }
}

impl<F, T> LocalTask<Reactor, F, T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    /// Constructs a new `LocalTask` from a [`Future`] to be scheduled on the current [`Reactor`].
    pub(crate) fn with_future(fut: F) -> Rc<Self> {
        Rc::new(Self {
            executor: Some(Reactor::current()),
            result: RefCell::new(ResultState::Empty),
            future: RefCell::new(fut),
        })
    }
}

impl<F, T> TaskBase for LocalTask<Reactor, F, T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    fn executor(&self) -> impl Executor + 'static {
        self.executor.unwrap()
    }
}

impl<E, F, T> RcTask for LocalTask<E, F, T>
where
    E: Executor + 'static,
    T: 'static,
    F: Future<Output = T> + 'static,
    LocalTask<E, F, T>: TaskBase,
{
    type Output = T;

    fn run(rc_self: &Rc<Self>) -> bool {
        match rc_self.future.try_borrow_mut() {
            Ok(mut fut) => {
                let waker = waker_ref(rc_self);
                let mut ctx = Context::from_waker(&waker);

                // SAFETY: The future is pinned in place for the duration of this function call, and
                // it is not moved or dropped until the future is complete. The future is also not
                // accessed from any other thread, so it is safe to create a pinned reference to it.
                let fut = unsafe { Pin::new_unchecked(&mut *fut) };

                match fut.poll(&mut ctx) {
                    Poll::Ready(result) => {
                        let res = rc_self.result.borrow_mut().set_result(result);

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

    fn poll_result(rc_self: &Rc<Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        rc_self.result.borrow_mut().poll_result(cx)
    }
}

/// Schedules a new asynchronous task to be executed on the current [`Thread`] and returns a
/// [`JoinHandle`] to await results.
pub(crate) fn spawn_on_current_thread<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let task = LocalTask::<Thread, F, T>::with_future(fut);

    RcTask::schedule_by_ref(&task);

    task.into()
}

/// Schedules a new asynchronous task to be executed on the current [`Reactor`] and returns a
/// [`JoinHandle`] to await results.
pub(crate) fn spawn_on_current_reactor<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let task = LocalTask::<Reactor, F, T>::with_future(fut);

    RcTask::schedule_by_ref(&task);

    task.into()
}
