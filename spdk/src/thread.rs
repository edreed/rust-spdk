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
    future::Future,
    mem::MaybeUninit,
};

use spdk_sys::{
    Errno,
    spdk_thread,
    
    to_result,

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
    spdk_thread_send_msg,
};

use crate::{
    runtime::CpuSet,
    task::{
        JoinHandle,
        RcTask,
        ThreadTask,
    },
};

/// An abstraction of a lightweight, stackless thread of execution.
#[derive(PartialEq)]
pub enum Thread {
    Borrowed(*mut spdk_thread),
    Owned(*mut spdk_thread),
}

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
    pub fn new(name: &CStr, cpuset: &CpuSet) -> Self {
        let t = unsafe {
            spdk_thread_create(name.as_ptr(), cpuset.as_ptr())
        };

        Self::Owned(t)
    }

    /// Returns the thread with the specified unique identifier.
    pub fn from_id(id: u64) -> Option<Self> {
        unsafe {
            let t = spdk_thread_get_by_id(id);

            if !t.is_null() {
                return Some(Self::Borrowed(t));
            }

            None
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
            let sthread = spdk_thread_get_app_thread();

            if !sthread.is_null() {
                return Some(Self::Borrowed(sthread));
            }

            None
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
            let sthread = spdk_get_thread();

            if !sthread.is_null() {
                return Some(Self::Borrowed(sthread));
            }

            None
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
        match self {
            Self::Borrowed(t) | Self::Owned(t) => *t
        }
    }

    /// Returns a pointer to the mutable underlying `spdk_thread` structure.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut spdk_thread {
        match self {
            Self::Borrowed(t) | Self::Owned(t) => *t
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

        Self::Borrowed(t as *mut _)
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
        struct Msg {
            msg_fn: Box<dyn FnOnce()>
        }
    
        unsafe extern "C" fn handle_msg(ctx: *mut c_void) {
            let msg = Box::from_raw(ctx as *mut Msg);
    
            (msg.msg_fn)();
        }

        let msg = Msg{ msg_fn: Box::new(f) };
        let ctx = Box::into_raw(Box::new(msg)).cast();

        unsafe {
            to_result!(spdk_thread_send_msg(self.as_ptr(), Some(handle_msg), ctx))
        }
    }

    /// Spawns a new asynchronous task to be executed on this thread and returns a
    /// [`JoinHandle`] to await results.
    pub fn spawn<F, T>(&self, fut: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static
    {
        let (task, join_handle) = ThreadTask::with_future(Some(self.borrow()), fut);

        RcTask::schedule(task);

        join_handle
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        if let Self::Owned(t) = self {
            unsafe { spdk_thread_exit(*t); }
        }
    }
}

/// Spawns a new asynchronous task to be executed on the current thread and
/// returns a [`JoinHandle`] to await results.
pub fn spawn<F, T>(fut: F) -> JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static
{
    Thread::current().spawn(fut)
}
