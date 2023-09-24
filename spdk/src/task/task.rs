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

use crate::thread::Thread;

use super::JoinHandle;


/// A way of scheduling and executing a [`LocalTask`] on the current [`Thread`].
pub(crate) trait RcTask: Send {
    /// Schedules a task for execution on its target thread, consuming the task
    /// in the process.
    fn schedule(rc_self: Rc<Self>);

    /// Schedules a task for execution on the current thread without consuming
    /// the task.
    /// 
    /// # Panics
    /// 
    /// This method panics if this task's target thread is not the current
    /// thread.
    fn schedule_by_ref(rc_self: &Rc<Self>);

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
    /// This method panics if this the current thread is not an SPDK thread.
    fn run(rc_self: &Rc<Self>) -> bool;
}

/// Encapsulates the execution state of a [`Future`].
pub(crate) struct TaskState<T: 'static> {
    future: Option<Pin<Box<dyn Future<Output = T> + 'static>>>,
    result_sender: Option<oneshot::Sender<T>>
}

/// Orchestrates the execution of a [`Future`].
pub(crate) struct Task<T: 'static> {
    target_thread: Option<Thread>,
    state: RefCell<TaskState<T>>,
}

unsafe impl <T: 'static> Send for Task<T> {}

impl <T: 'static> Task<T> {
    /// Constructs a new task from a [`Future`].
    /// 
    /// If `target_thread` is `None`, the task will be scheduled to run on the
    /// application thread.
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`LocalTask`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_future(
        target_thread: Option<Thread>,
        fut: impl Future<Output = T> + 'static
    ) -> (Rc<Task<T>>, JoinHandle<T>) {
        Self::with_boxed(target_thread, Box::pin(fut))
    }

    /// Constructs a new task from a [`BoxFuture`].
    /// 
    /// If `target_thread` is `None`, the task will be scheduled to run on the
    /// application thread.
    /// 
    /// # Return
    /// 
    /// This function returns both a newly constructed [`Task`] and a
    /// [`JoinHandle`] that can be used to await the result of executing the
    /// [`Future`].
    pub(crate) fn with_boxed(
        target_thread: Option<Thread>,
        boxed_fut: Pin<Box<dyn Future<Output = T> + 'static>>
    ) -> (Rc<Task<T>>, JoinHandle<T>) {
        let (sx, rx) = oneshot::channel();
        let task = Rc::new(Task{
            target_thread: target_thread,
            state: RefCell::new(TaskState {
                future: Some(boxed_fut),
                result_sender: Some(sx)
            })
        });

        (task, JoinHandle::new(rx))
    }

    /// Clones the [`RawWaker`] for this task.
    /// 
    /// This function is invoked through the [`RawWakerVTable`] created by the
    /// [`waker_ref`] function.
    /// 
    /// [`waker_ref`]: method@Self::waker_ref
    unsafe fn waker_clone(data: *const ()) -> RawWaker {
        Rc::<Self>::increment_strong_count(data.cast());

        RawWaker::new(data, Self::waker_vtable())
    }

    /// Wakes the [`LocalTask`] referenced by the given [`RawWaker`] consuming
    /// the `RawWaker` in the process.
    /// 
    /// This function is invoked through the [`RawWakerVTable`] created by the
    /// [`waker_ref`] function.
    /// 
    /// [`waker_ref`]: method@Self::waker_ref
    unsafe fn waker_wake(data: *const ()) {
        let rc_task = Rc::<Self>::from_raw(data.cast());

        RcTask::schedule(rc_task);
    }

    /// Wakes the [`LocalTask`] referenced by the given [`RawWaker`] without
    /// consuming the `RawWaker`.
    /// 
    /// This function is invoked through the [`RawWakerVTable`] created by the
    /// [`waker_ref`] function.
    /// 
    /// [`waker_ref`]: method@Self::waker_ref
    unsafe fn waker_wake_by_ref(data: *const ()) {
        let rc_task = ManuallyDrop::new(Rc::<Self>::from_raw(data.cast()));

        RcTask::schedule_by_ref(&rc_task);
    }

    /// Drops the given [`RawWaker`] releasing its resources.
    /// 
    /// This function is invoked through the [`RawWakerVTable`] created by the
    /// [`waker_ref`] function.
    /// 
    /// [`waker_ref`]: method@Self::waker_ref
    unsafe fn waker_drop(data: *const ()) {
        drop(Rc::<Self>::from_raw(data.cast()));
    }

    /// Gets the [`RawWakerVTable`] used by a [`RawWaker`] to awaken a [`LocalTask`].
    fn waker_vtable() -> &'static RawWakerVTable {
        &RawWakerVTable::new(
            Self::waker_clone,
            Self::waker_wake,
            Self::waker_wake_by_ref,
            Self::waker_drop,
        )
    }

    /// Creates a reference to the [`Waker`] from a reference to `Rc<LocalTask<T>>`.
    fn waker_ref(rc_self: &Rc<Self>) -> WakerRef<'_> {
        let data = Rc::as_ptr(rc_self).cast();

        let waker = ManuallyDrop::new(unsafe {
            Waker::from_raw(RawWaker::new(data, Self::waker_vtable()))
        });

        WakerRef::new_unowned(waker)
    }
}

impl <T: 'static> RcTask for Task<T> {
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
                let waker = Self::waker_ref(rc_self);
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