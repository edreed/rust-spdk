#![cfg(feature = "bdev")]
use std::{marker::PhantomData, ptr::NonNull};

use spdk_sys::spdk_thread;

use super::AsRawThread;

/// Represents an SPDK thread that has a lifetime owned by a specific type.
///
/// This type is used to scope the lifetime of a borrowed SPDK thread to the lifetime of its owner,
/// `T`.
pub struct OwnedBy<'a, T>(NonNull<spdk_thread>, PhantomData<&'a T>);

unsafe impl<'a, T> Send for OwnedBy<'a, T> {}
unsafe impl<'a, T> Sync for OwnedBy<'a, T> {}

impl<'a, T> OwnedBy<'a, T> {
    /// Creates a new `OwnedBy` instance from a raw `spdk_thread` pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the provided `spdk_thread` pointer is non-null and valid for the
    /// lifetime of the owner.
    pub(crate) unsafe fn with_owner(_owner: &'a T, thread: *mut spdk_thread) -> Self {
        Self(unsafe { NonNull::new_unchecked(thread) }, PhantomData)
    }
}

impl<'a, T> AsRawThread for OwnedBy<'a, T> {
    fn as_raw_thread(&self) -> *mut spdk_thread {
        self.0.as_ptr()
    }
}
