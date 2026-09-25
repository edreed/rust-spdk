//! Support for the Storage Performance Development Kit io_uring Block Device
//! plug-in.
use std::{ffi::CStr, mem::zeroed, ptr::NonNull, task::Poll};

use spdk_sys::{
    bdev_uring_opts, create_uring_bdev, delete_uring_bdev, spdk_bdev, spdk_bdev_get_name,
    spdk_uuid_copy,
};

use crate::{
    Result, Uuid,
    block::{Device, Owned, OwnedOps},
    errors::EINVAL,
    task::{Promise, Promissory},
};

/// Builds a [`Uring`] instance using the io_uring Block Device module of the
/// SPDK.
///
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chaining function calls. The [`build`] method constructs a new
/// `Uring` instance.
///
/// [`build`]: Builder::build
pub struct Builder(bdev_uring_opts);

unsafe impl Send for Builder {}

impl Builder {
    pub fn new() -> Self {
        // SAFETY: It is safe to initialize `bdev_uring_opts` with zeroed memory.
        Self(unsafe { zeroed() })
    }

    /// Sets the block device name.
    pub fn with_name(mut self, name: &CStr) -> Self {
        self.0.name = name.as_ptr();
        self
    }

    /// Sets the path to the backing file or block device.
    pub fn with_filename(mut self, path: &CStr) -> Self {
        self.0.filename = path.as_ptr();
        self
    }

    /// Sets the block size, in bytes, of the block device.
    ///
    /// If not specified or set to 0, the block size will be determined by the underlying block
    /// device.
    pub fn with_block_size(mut self, block_size: u32) -> Self {
        self.0.block_size = block_size;
        self
    }

    /// Sets the UUID of the device.
    ///
    /// If no UUID is explicitly set, a new UUID will be generated.
    pub fn with_uuid(mut self, uuid: &Uuid) -> Self {
        unsafe {
            spdk_uuid_copy(&mut self.0.uuid, uuid.as_ptr());
        }
        self
    }

    /// Creates a new [`Device<Uring>`] instance that owns the underlying
    /// `spdk_bdev` pointer.
    ///
    /// # Notes
    ///
    /// The returned [`Device<Uring>`] instance owns the underlying `spdk_bdev`
    /// pointer and will destroy it when dropped. See [`Device<T>`] for a detailed
    /// discussion of ownership semantics and requirements.
    pub fn build(self) -> Result<Device<Uring>> {
        let uring = unsafe { create_uring_bdev(&self.0) };

        NonNull::new(uring)
            .map(|ptr| Device::new(Uring(ptr)))
            .ok_or(EINVAL)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// A Linux io_uring-based BDev providing block layer access to a backing file or block device.
///
/// # Examples
///
/// ```no_run
#[doc = include_str!("../../examples/bdev_uring.rs")]
/// ```
pub struct Uring(NonNull<spdk_bdev>);

unsafe impl Send for Uring {}

impl OwnedOps for Uring {
    fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_ptr()
    }

    async fn destroy(self) -> Result<()> {
        Promise::new()
            .request(move |p| {
                let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

                unsafe {
                    delete_uring_bdev(
                        spdk_bdev_get_name(self.as_ptr()),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    );
                }

                Poll::Pending
            })
            .await
    }
}

impl From<Owned> for Uring {
    fn from(owned: Owned) -> Self {
        // SAFETY: We are creating a `Uring` instance from an `Owned` device, which guarantees that
        // the underlying `spdk_bdev` pointer is valid and uniquely owned.
        Self(unsafe { NonNull::new_unchecked(owned.into_ptr()) })
    }
}
