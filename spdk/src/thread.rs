//! An abstraction of a lightweight, stackless thread of execution.
//! 
//! An SPDK thread does not correspond 1:1 with a posix thread. Instead, a
//! lower-level framework like the SPDK Event Framework polls each SPDK thread
//! for work. This allows the SPDK Event Framework to multiplex many SPDK
//! threads on a smaller number of posix threads.
//! 
//! There are two mechanisms for scheduling work on an SPDK thread: messages and
//! pollers. A message consists of a function and single context parameter. A
//! poller is a function that is called periodically.
//! 
//! See [Message Passing and Concurrency] for more details on the SPDK
//! threading model.
//! 
//! [Message Passing and Concurrency]: https://spdk.io/doc/concurrency.html
use std::{
    ffi::{
        c_void,
        CStr,
    },
    fmt::{
        self,

        Debug,
        Formatter,
    },
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    ptr::NonNull,
};

use spdk_sys::{
    spdk_thread,

    spdk_cpuset_copy,
    spdk_get_thread,
    spdk_thread_bind,
    spdk_thread_create,
    spdk_thread_exit,
    spdk_thread_get_app_thread,
    spdk_thread_get_by_id,
    spdk_thread_get_cpumask,
    spdk_thread_get_id,
    spdk_thread_get_name,
    spdk_thread_is_bound,
    spdk_thread_is_running,
    spdk_thread_poll,
    spdk_thread_send_msg,
};

use crate::{
    errors::{
        Errno,

        ENOMEM,
    },
    runtime::CpuSet,
    task::{
        JoinHandle,
        Task,
        ThreadTask,
    },
    to_result,
};

/// Represents the ownership state of a [`Thread`].
#[derive(PartialEq)]
enum OwnershipState {
    Owned(NonNull<spdk_thread>),
    Borrowed(NonNull<spdk_thread>),
}

/// An abstraction of a lightweight, stackless thread of execution.
/// 
/// `Thread` wraps an `spdk_thread` pointer and can be in one of two ownership
/// states: owned or borrowed.
/// 
/// An owned thread owns the underlying `spdk_thread` pointer and will mark it
/// for exit when dropped. Any further processing requests on this thread will
/// fail.
/// 
/// A borrowed thread borrows the underlying `spdk_thread` pointer. Dropping a
/// borrowed thread has no effect on the underlying `spdk_thread` pointer.
#[derive(PartialEq)]
pub struct Thread(OwnershipState);

unsafe impl Send for Thread{}
unsafe impl Sync for Thread{}

impl Thread {
    /// Creates a new owned thread.
    /// 
    /// # Notes
    /// 
    /// The thread object returned is owned by the caller. When dropped, the
    /// thread will be marked for exit causing any further processing requests
    /// on this thread to fail.
    pub fn new(name: &CStr, cpuset: &CpuSet) -> Result<Self, Errno> {
        let t = unsafe {
            spdk_thread_create(name.as_ptr(), cpuset.as_ptr())
        };

        match NonNull::new(t) {
            Some(t) => Ok(Self(OwnershipState::Owned(t))),
            None => Err(ENOMEM),
        }
    }

    /// Returns an owned thread object for the specified pointer.
    pub fn from_ptr_owned(thread: *mut spdk_thread) -> Self {
        match NonNull::new(thread) {
            Some(t) => Self(OwnershipState::Owned(t)),
            None => panic!("thread pointer must not be null"),
        }
    }

    /// Returns a borrowed thread object for the specified pointer.
    pub fn from_ptr(thread: *mut spdk_thread) -> Self {
        match NonNull::new(thread) {
            Some(t) => Self(OwnershipState::Borrowed(t)),
            None => panic!("thread pointer must not be null"),
        }
    }

    /// Returns a borrowed thread object for the specified non-null pointer.
    ///
    /// # Safety
    /// 
    /// The caller must ensure that `thread` is non-null and points to a valid
    /// `spdk_thread` object.
    pub unsafe fn from_ptr_unchecked(thread: *mut spdk_thread) -> Self {
        Self(OwnershipState::Borrowed(NonNull::new_unchecked(thread)))
    }

    /// Returns the borrowed thread with the specified unique identifier.
    pub fn from_id(id: u64) -> Option<Self> {
        unsafe {
            let t = spdk_thread_get_by_id(id);

            match NonNull::new(t) {
                Some(t) => Some(Self(OwnershipState::Borrowed(t))),
                None => None,
            }
        }
    }

    /// Tries to return the application thread object.
    /// 
    /// The application thread is the thread that initialized the SPDK Application
    /// Framework.
    /// 
    /// # Return
    /// 
    /// If the Application Framework has been initialized, this function returns
    /// `Some(t)` where `t` is the SPDK application thread object. Otherwise, this
    /// function returns `None`.
    pub fn try_application() -> Option<Self> {
        unsafe {
            let t = spdk_thread_get_app_thread();

            match NonNull::new(t) {
                Some(t) => Some(Self(OwnershipState::Borrowed(t))),
                None => None,
            }
        }
    }

    /// Returns the application thread object.
    /// 
    /// The application thread is the thread that initialized the SPDK Application
    /// Framework.
    /// 
    /// # Panics
    /// 
    /// This function panics if the SPDK Application Framework has not been
    /// initialized.
    pub fn application() -> Self {
        Self::try_application().expect("SPDK Application Framework must be initialized")
    }

    /// Tries to return the current thread object.
    /// 
    /// # Return
    /// 
    /// If the current system thread is an SPDK thread, this function returns
    /// `Some(t)` where `t` is the current SPDK thread object. Otherwise, this
    /// function returns `None`.
    pub fn try_current() -> Option<Self> {
        unsafe {
            let t = spdk_get_thread();

            match NonNull::new(t) {
                Some(t) => Some(Self(OwnershipState::Borrowed(t))),
                None => None,
            }
        }
    }

    /// Returns the current thread object.
    /// 
    /// # Panics
    /// 
    /// This function panics if the current system thread is not an SPDK thread.
    pub fn current() -> Self {
        Self::try_current().expect("must be called on SPDK thread")
    }

    /// Returns whether this thread is the current thread.
    pub fn is_current(&self) -> bool {
        if let Some(t) = Self::try_current() {
            return *self == t;
        }

        false
    }

    /// Returns a pointer to the underlying `spdk_thread` structure.
    pub(crate) fn as_ptr(&self) -> *const spdk_thread {
        match self.0 {
            OwnershipState::Borrowed(t) | OwnershipState::Owned(t) => t.as_ptr()
        }
    }

    /// Returns a pointer to the mutable underlying `spdk_thread` structure.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut spdk_thread {
        match self.0 {
            OwnershipState::Borrowed(t) | OwnershipState::Owned(t) => t.as_ptr()
        }
    }

    /// Borrows the thread object.
    /// 
    /// # Notes
    /// 
    /// Dropping the borrow has no effect on the underlying thread object.
    pub fn borrow(&self) -> Self {
        let t = self.as_ptr();

        assert!(
            unsafe { spdk_thread_is_running(t as *mut _) },
            "thread {} is no longer running",
            self.name().to_string_lossy());

        let t = unsafe { NonNull::new_unchecked(t as *mut _) };

        Self(OwnershipState::Borrowed(t))
    }

    /// Returns the name of this thread.
    pub fn name(&self) -> &'static CStr {
        unsafe {
            let name = spdk_thread_get_name(self.as_ptr());

            CStr::from_ptr(name)
        }
    }

    /// Returns the unique identifier for this thread.
    pub fn id(&self) -> u64 {
        unsafe { spdk_thread_get_id(self.as_ptr()) }
    }

    /// Bind or unbind the thread to its current CPU core.
    pub fn bind(&mut self, bind: bool) {
        unsafe { spdk_thread_bind(self.as_mut_ptr(), bind) }
    }

    /// Returns whether the thread is bound to its current CPU core.
    pub fn is_bound(&self) -> bool {
        unsafe { spdk_thread_is_bound(self.as_ptr() as *mut _) }
    }

    /// Returns the CPU set for this thread.
    pub fn cpuset(&self) -> CpuSet {
        unsafe {
            let mut cpuset = MaybeUninit::uninit();

            spdk_cpuset_copy(cpuset.as_mut_ptr(), spdk_thread_get_cpumask(self.as_ptr() as *mut _));

            cpuset.assume_init().into()
        }
    }

    /// Invokes a function sent via [`Thread::send_msg()`] on the current thread.
    /// 
    /// [`Thread::send_msg()`]: method@Thread::send_msg
    unsafe extern "C" fn handle_msg<F>(ctx: *mut c_void)
    where
        F: FnOnce() + 'static
    {
        let msg_fn = Box::from_raw(ctx as *mut F);

        (*msg_fn)();
    }

    /// Sends a message function to be executed on this thread.
    /// 
    /// The message is sent asynchronously. This function may return before the
    /// message function is called.
    /// 
    /// # Return
    /// 
    /// This function returns `Ok(())` if the message function was successfully
    /// queued.
    /// 
    /// This function return [`ENOMEM`] if the message could not be allocated
    /// and [`EIO`] if the message could not be sent to the destination thread.
    /// 
    /// # Examples
    /// 
    /// ```no_run
    /// use spdk::thread::Thread;
    /// 
    /// let t = Thread::current();
    /// 
    /// assert!(t.send_msg(|| println!("Hello, World!")).is_ok());
    /// ```
    /// 
    /// [`EIO`]: crate::errors::EIO
    /// [`ENOMEM`]: crate::errors::ENOMEM
    pub fn send_msg<F>(&self, f: F) -> Result<(), Errno>
    where
        F: FnOnce() + 'static
     {
        let ctx = Box::into_raw(Box::new(f)).cast();

        unsafe {
            to_result!(spdk_thread_send_msg(self.as_ptr(), Some(Self::handle_msg::<F>), ctx))
        }
    }

    /// Perform one iteration worth of processing on the thread.
    /// 
    /// # Returns
    /// 
    /// This method returns `true` if work was done and `false` otherwise.
    pub(crate) fn poll(&self) -> bool {
        unsafe { spdk_thread_poll(self.as_ptr() as *mut _, 0, 0) != 0 }
    }

    /// Spawns a new asynchronous task to be executed on this thread and returns a
    /// [`JoinHandle`] to await results.
    /// 
    /// The indirection of `fut_gen` instead of receiving a `Future` directly
    /// allows for futures that may not be `Send` once started.
    pub fn spawn<G, F, T>(&self, fut_gen: G ) -> JoinHandle<T>
    where
        G: FnOnce() -> F + Send + 'static,
        F: Future<Output = T> + 'static,
        T: Send + 'static
    {
        let (task, join_handle) = ThreadTask::with_future(Some(self.borrow()), fut_gen());

        Task::schedule(task);

        join_handle
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        if let OwnershipState::Owned(t) = self.0 {
            unsafe { spdk_thread_exit(t.as_ptr()); }
        }
    }
}

impl Debug for Thread {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Thread(\"{}\")", self.name().to_string_lossy())
    }
}

impl From<*mut spdk_thread> for Thread {
    fn from(value: *mut spdk_thread) -> Self {
        Thread::from_ptr(value)
    }
}

/// Spawns a new asynchronous task to be executed on the current SPDK thread and
/// returns a [`JoinHandle`] to await results.
pub fn spawn_local<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + 'static,
    T: 'static
{
    let (task, join_handle) = ThreadTask::with_future(Some(Thread::current()), fut);

    Task::schedule(task);

    join_handle
}

/// Runs the provided future on the current SPDK thread until completion.
/// 
/// # Notes
/// 
/// This function blocks the current reactor until the future completes on the
/// current SPDK thread. Although the given future may spawn concurrent tasks on
/// this thread, tasks on other threads associated with the current reactor will
/// not run. The given future must not depend on the result of concurrent tasks
/// associated with other threads, otherwise a deadlock will occur.
pub fn block_on<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static
{
    let current_thread = Thread::current();
    let (task, mut join_handle) = 
        ThreadTask::with_future(Some(current_thread.borrow()), fut);

    Task::schedule_by_ref(&task);

    loop {
        match Pin::new(&mut join_handle).rx_pin_mut().try_recv() {
            Ok(Some(r)) => return r,
            Ok(None) => _ = current_thread.poll(),
            Err(_) => panic!("sender dropped"),
        }
    }
}
