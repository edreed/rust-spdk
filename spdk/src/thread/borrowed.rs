use std::ptr::NonNull;

use spdk_sys::{spdk_get_thread, spdk_thread};

use crate::thread::AsRawThread;

/// Represents a borrowed SPDK thread.
///
/// This type does not own the underlying `spdk_thread` pointer and dropping it has no effect on the
/// actual thread.
pub struct Borrowed(NonNull<spdk_thread>);

impl Borrowed {
    /// Tries to return the current thread object.
    ///
    /// # Return
    ///
    /// If the current system thread is an SPDK thread, this function returns `Some(Self)`.
    /// Otherwise, this function returns `None`.
    pub(crate) fn try_current() -> Option<Self> {
        NonNull::new(unsafe { spdk_get_thread() }).map(Self)
    }

    pub(crate) unsafe fn new_unchecked(thread: *mut spdk_thread) -> Self {
        Self(unsafe { NonNull::new_unchecked(thread) })
    }
}

impl AsRawThread for Borrowed {
    fn as_raw_thread(&self) -> *mut spdk_thread {
        self.0.as_ptr()
    }
}
