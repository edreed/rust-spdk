use std::{
    cell::RefCell,
    fmt::Debug,
    mem::{
        self,

        ManuallyDrop,
    },
    pin::Pin,
    sync::{
        Arc,
        Mutex,
    },
    task::{
        Context,
        Poll,
        RawWaker,
        RawWakerVTable,
        Waker,
    },
};

use futures::{
    channel::oneshot,
    Future,
    task::WakerRef,
};

use crate::{
    runtime::Reactor,
    thread::Thread,
};

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

    /// Executes a task on the executor.
    /// 
    /// # Returns
    /// 
    /// This method returns `true` if the task was run synchronously. If it
    /// returns `false`, the task could not be executed synchronously and
    /// should be scheduled to run later.
    fn run(arc_self: &Arc<Self>) -> bool;
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

/// Creates a reference to the [`Waker`] from a reference to an [`RcTask<T>`].
fn waker_ref<W: Task>(arc_self: &Arc<W>) -> WakerRef<'_> {
    let data = Arc::as_ptr(arc_self).cast();

    let waker = ManuallyDrop::new(unsafe {
        Waker::from_raw(RawWaker::new(data, waker_vtable::<W>()))
    });

    WakerRef::new_unowned(waker)
}

/// Encapsulates the execution state of a [`Future`].
enum TaskState<T: 'static> {
    /// The task is pending execution.
    Pending(Pin<Box<dyn Future<Output = T> + 'static>>),

    /// The task is currently being polled.
    Polling,

    /// The task has completed execution.
    Done,
}

impl <T: 'static> TaskState<T> {
    /// Replaces the current state with the specified value and returns the old state.
    fn replace(&mut self, value: Self) -> Self {
        mem::replace(&mut *self, value)
    }

    /// Polls the state of the task, advancing to the next state if possible.
    fn poll(&mut self) -> Self{
        match self {
            Self::Pending(_) => self.replace(Self::Polling),
            Self::Polling => Self::Polling,
            _ => panic!("task polled in unexpected state: {:?}", self),
        }
    }
}

impl <T: 'static> Debug for TaskState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending(_) => write!(f, "Pending"),
            Self::Polling => write!(f, "Polling"),
            Self::Done => write!(f, "Done"),
        }
    }
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct ThreadTask<T: 'static> {
    target_thread: Option<Thread>,
    result_sender: RefCell<Option<oneshot::Sender<T>>>,
    state: Mutex<TaskState<T>>,
}

unsafe impl <T: 'static> Send for ThreadTask<T> {}

impl <T: 'static> ThreadTask<T> {
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
    pub(crate) fn with_future(
        target_thread: Option<Thread>,
        fut: impl Future<Output = T> + 'static
    ) -> (Arc<Self>, JoinHandle<T>) {
        Self::with_boxed(target_thread, Box::pin(fut))
    }

    /// Constructs a new task from a [`Pin<Box<Future>>`] to be scheduled on a
    /// [`Thread`].
    /// 
    /// If `target_thread` is `None`, the task will be scheduled to run on the
    /// application thread.
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`ThreadTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_boxed(
        target_thread: Option<Thread>,
        boxed_fut: Pin<Box<dyn Future<Output = T> + 'static>>
    ) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self {
            target_thread: target_thread,
            result_sender: RefCell::new(Some(sx)),
            state: Mutex::new(TaskState::Pending(boxed_fut)),
        });

        (task, JoinHandle::new(rx))
    }
}

impl <T: 'static> Task for ThreadTask<T> {
    fn schedule(arc_self: Arc<Self>) {
        let target_thread = arc_self.target_thread
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(|| Thread::application());

        // If the current thread is the target of this task, attempt to run it
        // synchronously.
        if let Some(t) = Thread::try_current() {
            if t == target_thread {
                if Self::run(&arc_self) {
                    return;
                }
            }
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this thread at a later time.
        target_thread.send_msg(move || assert!(Task::run(&arc_self))).unwrap();
    }

    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let target_thread = arc_self.target_thread
            .as_ref()
            .map(Thread::borrow)
            .unwrap_or_else(|| Thread::application());

        assert!(target_thread.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(arc_self) {
            // The task could not be run synchronously, so enqueue it to run on the
            // this thread at a later time.
            let cloned_task = arc_self.clone();

            target_thread.send_msg(move || assert!(Task::run(&cloned_task))).unwrap();
        }
    }

    fn run(arc_self: &Arc<Self>) -> bool {
        let state = arc_self.state.lock().unwrap().poll();

        match state {
            TaskState::Pending(mut fut) => {
                let waker = waker_ref(arc_self);
                let ctx = &mut Context::from_waker(&waker);

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => {
                        *arc_self.state.lock().unwrap() = TaskState::Pending(fut);
                    },
                    Poll::Ready(r) => {
                        let _ = arc_self.result_sender.borrow_mut().take().unwrap().send(r);
                        *arc_self.state.lock().unwrap() = TaskState::Done;
                    },
                }

                true
            },
            TaskState::Polling => false,
            _ => unreachable!()
        }
    }
}

/// Orchestrates the execution of a [`Future`] on a reactor.
pub(crate) struct ReactorTask<T: 'static> {
    target_reactor: Reactor,
    result_sender: RefCell<Option<oneshot::Sender<T>>>,
    state: Mutex<TaskState<T>>,
}

unsafe impl <T: 'static> Send for ReactorTask<T> {}

impl <T: 'static> ReactorTask<T> {
    /// Constructs a new task from a [`Future`] to be scheduled on the given
    /// [`Reactor`]
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`ReactorTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_future(
        target_reactor: Reactor,
        fut: impl Future<Output = T> + 'static
    ) -> (Arc<Self>, JoinHandle<T>) {
        Self::with_boxed(target_reactor, Box::pin(fut))
    }

    /// Constructs a new task from a [`Pin<Box<Future>>`] to be scheduled on the
    /// given [`Reactor`]
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`ReactorTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_boxed(
        target_reactor: Reactor,
        boxed_fut: Pin<Box<dyn Future<Output = T> + 'static>>
    ) -> (Arc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Arc::new(Self{
            target_reactor: target_reactor,
            result_sender: RefCell::new(Some(sx)),
            state: Mutex::new(TaskState::Pending(boxed_fut)),
        });

        (task, JoinHandle::new(rx))
    }
}

impl <T: 'static> Task for ReactorTask<T> {
    fn schedule(arc_self: Arc<Self>) {
        let target_reactor = arc_self.target_reactor;

        // If the current reactor is the target of this task, attempt to run it
        // synchronously.
        if let Some(r) = Reactor::try_current() {
            if r == target_reactor {
                if Self::run(&arc_self) {
                    return;
                }
            }
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this reactor at a later time.
        target_reactor.send_event(move || assert!(Task::run(&arc_self))).unwrap();
    }

    fn schedule_by_ref(arc_self: &Arc<Self>) {
        let target_reactor = arc_self.target_reactor;

        assert!(target_reactor.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(arc_self) {
            // The task could not be run synchronously, so enqueue it to run on
            // the this reactor at a later time.
            let cloned_task = arc_self.clone();

            target_reactor.send_event(move || assert!(Task::run(&cloned_task))).unwrap();
        }
    }

    fn run(arc_self: &Arc<Self>) -> bool {
        let state = arc_self.state.lock().unwrap().poll();

        match state {
            TaskState::Pending(mut fut) => {
                let waker = waker_ref(arc_self);
                let ctx = &mut Context::from_waker(&waker);

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => {
                        *arc_self.state.lock().unwrap() = TaskState::Pending(fut);
                    },
                    Poll::Ready(r) => {
                        let _ = arc_self.result_sender.borrow_mut().take().unwrap().send(r);
                        *arc_self.state.lock().unwrap() = TaskState::Done;
                    },
                }

                true
            },
            TaskState::Polling => false,
            _ => unreachable!()
        }
    }
}
