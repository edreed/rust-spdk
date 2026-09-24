use std::{
    ffi::{CStr, CString},
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr::{NonNull, null},
    task::Poll,
};

use spdk_sys::{
    bdev_aio_delete, create_aio_bdev, spdk_bdev, spdk_bdev_get_by_name, spdk_bdev_get_name,
};

use crate::{
    Result,
    block::{Device, Owned, OwnedOps},
    errors::EINVAL,
    task::{Promise, Promissory},
    to_result,
    uuid::Uuid,
};

/// A Linux AIO-based BDev providing block layer access to a backing file or block device.
///
/// # Examples
/// ```no_run
#[doc = include_str!("../../examples/bdev_aio.rs")]
/// ```
pub struct Aio(NonNull<spdk_bdev>);

unsafe impl Send for Aio {}

impl Aio {
    /// Create a new AIO block device.
    ///
    /// # Parameters
    ///
    /// - `name`: The name of the AIO block device.
    /// - `filename`: The path to the backing file or block device.
    /// - `block_size`: The block size in bytes. If not specified, the block size will be detected
    ///   from the device specified in `filename`.
    /// - `readonly`: Whether the file or device should be opened read-only.
    /// - `falloc`: Whether to preallocate the file.
    /// - `uuid`: The UUID of the device. If not specifed, a unique UUID will be generated.
    /// - `nowait`: If `true`, do not wait if I/O would block for any reason. This may only be
    ///   applied to block devices.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing a [`Device<Aio>`] instance that owns the newly created BDev if successful.
    pub fn new<P: AsRef<Path>>(
        name: &CStr,
        filename: P,
        block_size: Option<u32>,
        readonly: bool,
        falloc: bool,
        uuid: Option<Uuid>,
        nowait: bool,
    ) -> Result<Device<Aio>> {
        let filename =
            CString::new(filename.as_ref().as_os_str().as_bytes()).map_err(|_| EINVAL)?;

        unsafe {
            to_result!(create_aio_bdev(
                name.as_ptr(),
                filename.as_ptr(),
                block_size.unwrap_or(0),
                readonly,
                falloc,
                uuid.as_ref().map(Uuid::as_ptr).unwrap_or(null()),
                nowait
            ))?
        }

        match NonNull::new(unsafe { spdk_bdev_get_by_name(name.as_ptr()) }) {
            Some(bdev) => Ok(Device::new(Aio(bdev))),
            None => Err(EINVAL),
        }
    }
}

impl OwnedOps for Aio {
    fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_ptr()
    }

    async fn destroy(self) -> Result<()> {
        Promise::new()
            .request(move |p| {
                let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

                unsafe {
                    bdev_aio_delete(
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

impl From<Owned> for Aio {
    fn from(owned: Owned) -> Self {
        // SAFETY: We are creating an `Aio` instance from an `Owned` device, which guarantees that
        // the underlying `spdk_bdev` pointer is valid and uniquely owned.
        Self(unsafe { NonNull::new_unchecked(owned.as_ptr()) })
    }
}
