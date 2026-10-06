use std::{mem::transmute, ptr::NonNull};

use spdk_sys::spdk_thread;

use crate::thread::AsRawThread;

/// Represents an SPDK thread that has a lifetime not under the control of this instance.
///
/// This type does not own the underlying `spdk_thread` pointer and dropping it has no effect on the
/// actual thread.
#[repr(transparent)]
pub struct Unowned(NonNull<spdk_thread>);

impl Unowned {
    /// Creates a new `Unowned` instance if the provided pointer is non-null.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the provided pointer is non-null, points to a valid
    /// `spdk_thread` and is not used after the SPDK thread becomes invalid.
    pub(crate) unsafe fn new(thread: *mut spdk_thread) -> Option<Self> {
        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_thread>`.
        unsafe { transmute(NonNull::new(thread)) }
    }

    /// Creates a new `Unowned` instance without checking the validity of the pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the provided pointer is non-null, points to a valid
    /// `spdk_thread` and is not used after the SPDDK thread becomes invalid.
    pub(crate) unsafe fn new_unchecked(thread: *mut spdk_thread) -> Self {
        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_thread>`.
        unsafe { transmute(NonNull::new_unchecked(thread)) }
    }
}

impl AsRawThread for Unowned {
    fn as_raw_thread(&self) -> *mut spdk_thread {
        self.0.as_ptr()
    }
}
