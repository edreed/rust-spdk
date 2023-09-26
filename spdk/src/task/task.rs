use std::{
    cell::RefCell,
    mem::ManuallyDrop,
    pin::Pin,
    rc::Rc,
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
pub(crate) trait RcTask: Send {
    /// Schedules a task for execution on its target, consuming the task in the
    /// process.
    fn schedule(rc_self: Rc<Self>);

    /// Schedules a task for execution without consuming the task.
    /// 
    /// # Panics
    /// 
    /// This method panics if this task's target executor is not the current
    /// executor.
    fn schedule_by_ref(rc_self: &Rc<Self>);

    /// Executes a task on the executor.
    /// 
    /// # Returns
    /// 
    /// This method returns `true` if the task was run synchronously. If it
    /// returns `false`, the task could not be executed synchronously and
    /// should be scheduled to run later.
    fn run(rc_self: &Rc<Self>) -> bool;
}

/// Clones the [`RawWaker`] for this task.
/// 
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_clone<W: RcTask>(data: *const ()) -> RawWaker {
    Rc::<W>::increment_strong_count(data.cast());

    RawWaker::new(data, waker_vtable::<W>())
}

/// Wakes the task referenced by the given [`RawWaker`] consuming the `RawWaker`
/// in the process.
/// 
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_wake<W: RcTask>(data: *const ()) {
    let rc_task = Rc::<W>::from_raw(data.cast());

    RcTask::schedule(rc_task);
}

/// Wakes the task referenced by the given [`RawWaker`] without consuming the
/// `RawWaker`.
/// 
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_wake_by_ref<W: RcTask>(data: *const ()) {
    let rc_task = ManuallyDrop::new(Rc::<W>::from_raw(data.cast()));

    RcTask::schedule_by_ref(&rc_task);
}

/// Drops the given [`RawWaker`] releasing its resources.
/// 
/// This function is invoked through the [`RawWakerVTable`] created by the
/// [`waker_ref<W>`] function.
unsafe fn waker_drop<W: RcTask>(data: *const ()) {
    drop(Rc::<W>::from_raw(data.cast()));
}

/// Gets the [`RawWakerVTable`] used by a [`RawWaker`] to awaken a task.
fn waker_vtable<W: RcTask>() -> &'static RawWakerVTable {
    &RawWakerVTable::new(
        waker_clone::<W>,
        waker_wake::<W>,
        waker_wake_by_ref::<W>,
        waker_drop::<W>,
    )
}

/// Creates a reference to the [`Waker`] from a reference to an [`RcTask<T>`].
fn waker_ref<W: RcTask>(rc_self: &Rc<W>) -> WakerRef<'_> {
    let data = Rc::as_ptr(rc_self).cast();

    let waker = ManuallyDrop::new(unsafe {
        Waker::from_raw(RawWaker::new(data, waker_vtable::<W>()))
    });

    WakerRef::new_unowned(waker)
}

/// Encapsulates the execution state of a [`Future`].
pub(crate) struct TaskState<T: 'static> {
    future: Option<Pin<Box<dyn Future<Output = T> + 'static>>>,
    result_sender: Option<oneshot::Sender<T>>
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct ThreadTask<T: 'static> {
    target_thread: Option<Thread>,
    state: RefCell<TaskState<T>>,
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
    ) -> (Rc<Self>, JoinHandle<T>) {
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
    ) -> (Rc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Rc::new(Self {
            target_thread: target_thread,
            state: RefCell::new(TaskState {
                future: Some(boxed_fut),
                result_sender: Some(sx)
            })
        });

        (task, JoinHandle::new(rx))
    }
}

impl <T: 'static> RcTask for ThreadTask<T> {
    fn schedule(rc_self: Rc<Self>) {
        let target_thread = rc_self.target_thread.unwrap_or_else(|| Thread::application());

        // If the current thread is the target of this task, attempt to run it
        // synchronously.
        if let Some(t) = Thread::try_current() {
            if t == target_thread {
                if Self::run(&rc_self) {
                    return;
                }
            }
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this thread at a later time.
        target_thread.send_msg(move || assert!(RcTask::run(&rc_self))).unwrap();
    }

    fn schedule_by_ref(rc_self: &Rc<Self>) {
        let target_thread = rc_self.target_thread.unwrap_or_else(|| Thread::application());

        assert!(target_thread.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(rc_self) {
            // The task could not be run synchronously, so enqueue it to run on the
            // this thread at a later time.
            let cloned_task = rc_self.clone();

            target_thread.send_msg(move || assert!(RcTask::run(&cloned_task))).unwrap();
        }
    }

    fn run(rc_self: &Rc<Self>) -> bool {
        if let Ok(mut task_state) = rc_self.state.try_borrow_mut() {
            if let Some(mut fut) = task_state.future.take() {
                let waker = waker_ref(rc_self);
                let ctx = &mut Context::from_waker(&waker);

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => task_state.future = Some(fut),
                    Poll::Ready(r) => if let Some(s) = task_state.result_sender.take() {
                        _ = s.send(r);
                    },
                }
                return true
            }
        }

        false
    }
}

/// Orchestrates the execution of a [`Future`] on a reactor.
pub(crate) struct ReactorTask<T: 'static> {
    target_reactor: Reactor,
    state: RefCell<TaskState<T>>,
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
    ) -> (Rc<Self>, JoinHandle<T>) {
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
    ) -> (Rc<Self>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Rc::new(Self{
            target_reactor: target_reactor,
            state: RefCell::new(TaskState {
                future: Some(boxed_fut),
                result_sender: Some(sx)
            })
        });

        (task, JoinHandle::new(rx))
    }
}

impl <T: 'static> RcTask for ReactorTask<T> {
    fn schedule(rc_self: Rc<Self>) {
        let target_reactor = rc_self.target_reactor;

        // If the current reactor is the target of this task, attempt to run it
        // synchronously.
        if let Some(r) = Reactor::try_current() {
            if r == target_reactor {
                if Self::run(&rc_self) {
                    return;
                }
            }
        }

        // The task could not be run synchronously, so enqueue it to run on the
        // this reactor at a later time.
        target_reactor.send_event(move || assert!(RcTask::run(&rc_self))).unwrap();
    }

    fn schedule_by_ref(rc_self: &Rc<Self>) {
        let target_reactor = rc_self.target_reactor;

        assert!(target_reactor.is_current());

        // First, attempt to run the task synchronously.
        if !Self::run(rc_self) {
            // The task could not be run synchronously, so enqueue it to run on
            // the this reactor at a later time.
            let cloned_task = rc_self.clone();

            target_reactor.send_event(move || assert!(RcTask::run(&cloned_task))).unwrap();
        }
    }

    fn run(rc_self: &Rc<Self>) -> bool {
        if let Ok(mut task_state) = rc_self.state.try_borrow_mut() {
            if let Some(mut fut) = task_state.future.take() {
                let waker = waker_ref(rc_self);
                let ctx = &mut Context::from_waker(&waker);

                match fut.as_mut().poll(ctx) {
                    Poll::Pending => task_state.future = Some(fut),
                    Poll::Ready(r) => if let Some(s) = task_state.result_sender.take() {
                        _ = s.send(r);
                    },
                }
                return true
            }
        }

        false
    }
}
