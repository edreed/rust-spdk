use std::{ffi::CStr, ptr::NonNull};

use spdk_sys::{
    spdk_bdev, spdk_bdev_close, spdk_bdev_desc, spdk_bdev_desc_get_bdev, spdk_bdev_get_name,
};

use crate::Result;

use super::{AsRawBDev, descriptor::open_desc};

/// A placeholder type used with [`Device`] that represents any block device.
///
/// This type is used as a generic placeholder for any block device, without taking ownership of it.
/// In order to keep the `spdk_bdev` alive for the lifetime of the `Any` instance, the constructor
/// function opens a read-only descriptor to the block device and stores it in the `Any` instance.
/// This may fail if opening the read-only descriptor to the block device fails. Users of this
/// object should be prepared to handle these errors and fail gracefully.
///
/// [`Device`]: super::Device
pub struct Any(NonNull<spdk_bdev_desc>);

unsafe impl Send for Any {}
unsafe impl Sync for Any {}

impl Any {
    /// Create an `Any` instance from a raw `spdk_bdev` pointer without checking if it is null.
    ///
    /// # Returns
    ///
    /// This method returns an error if it fails to open a descriptor on the block device, and
    /// `Ok(any)` otherwise.
    ///
    /// # Safety
    ///
    /// `bdev` must be non-null and a pointer to a valid `spdk_bdev` structure.
    pub(crate) async unsafe fn from_ptr_unchecked(bdev: *mut spdk_bdev) -> Result<Any> {
        Self::with_name(unsafe { CStr::from_ptr(spdk_bdev_get_name(bdev)) }).await
    }

    /// Create an `Any` instance from the name of a block device.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(any)` if a block device with the given name exists and a descriptor
    /// can be successfully opened. It returns an error if it fails to open a descriptor on
    /// the block device.
    pub(crate) async fn with_name(name: &CStr) -> Result<Any> {
        open_desc(name, false).await.map(Any)
    }
}

impl Drop for Any {
    fn drop(&mut self) {
        // SAFETY: The pointer is guaranteed to be valid as long as the Any instance exists.
        unsafe { spdk_bdev_close(self.0.as_ptr()) };
    }
}

impl AsRawBDev for Any {
    fn as_raw_bdev(&self) -> *mut spdk_bdev {
        unsafe { spdk_bdev_desc_get_bdev(self.0.as_ptr()) }
    }
}
