use std::{ffi::c_void};

use spdk_sys::{
    Errno,
    spdk_get_thread,
    spdk_thread,
    spdk_thread_send_msg,
    to_result,
};

/// A lightweight, stackless thread of execution.
pub struct Thread(*mut spdk_thread);

impl Thread {
    /// Tries to return the current thread object.
    /// 
    /// # Return
    /// 
    /// If the current system thread is an SPDK thread, this function returns
    /// `Some(t)` where `t` is the current SPDK thread object. Otherwise, this
    /// funciton returns `None`.
    pub fn try_current() -> Option<Self> {
        unsafe {
            let sthread = spdk_get_thread();

            if !sthread.is_null() {
                return Some(Thread(sthread));
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
        Self::try_current().expect("must be called on spdk thread")
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
            to_result!(spdk_thread_send_msg(self.0, Some(handle_msg), ctx))
        }
    }
}
