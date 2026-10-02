use std::{ffi::CStr, os::raw::c_void, ptr::NonNull};

use spdk_sys::{spdk_thread, spdk_thread_create, spdk_thread_exit, spdk_thread_send_msg};

use crate::{Result, errors::ENOMEM, runtime::CpuSet, to_result};

use super::AsRawThread;

/// Represents an SPDK thread with lifetime ownership. That is, the thread is signaled to exit when
/// dropped.
pub struct Owned(NonNull<spdk_thread>);

unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}

impl Owned {
    pub(crate) fn new(name: &CStr, cpuset: &CpuSet) -> Result<Self> {
        NonNull::new(unsafe { spdk_thread_create(name.as_ptr(), cpuset.as_ptr()) })
            .map(Self)
            .ok_or(ENOMEM)
    }
}

impl AsRawThread for Owned {
    fn as_raw_thread(&self) -> *mut spdk_thread {
        self.0.as_ptr()
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: The Owned struct guarantees that the pointer is valid.
        unsafe extern "C" fn exit_thread(ctx: *mut c_void) {
            let _ = unsafe { spdk_thread_exit(ctx as *mut spdk_thread) };
        }

        // SAFETY: The `spdk_thread_exit` function must be called from a poller or thread
        // message. We dispatch the call via thread message to ensure this invariant is
        // satisfied.
        to_result!(unsafe {
            spdk_thread_send_msg(
                self.as_raw_thread(),
                Some(exit_thread),
                self.as_raw_thread() as *mut c_void,
            )
        })
        .expect("thread exit message sent");
    }
}
