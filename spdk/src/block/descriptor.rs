use std::{
    ffi::CStr,
    ptr::{
        NonNull,

        null_mut,
    },
};

use spdk_sys::{
    spdk_bdev,
    spdk_bdev_desc,

    spdk_bdev_close,
    spdk_bdev_desc_get_bdev,
    spdk_bdev_open_ext,
};

use crate::{
    bdev::Any,
    errors::Errno,
    to_result,
};

use super::{
    Device,
    IoChannel,
};

/// A handle to an open block device.
pub struct Descriptor(NonNull<spdk_bdev_desc>);

unsafe impl Send for Descriptor {}

impl Descriptor {
    /// Open a block device by its name.
    pub async fn open(name: &CStr, write: bool) -> Result<Descriptor, Errno> {
        unsafe extern "C" fn handle_event(_type: u32, _bdev: *mut spdk_bdev, _ctx: *mut std::ffi::c_void) {
        }

        let mut desc = null_mut();

        unsafe {
            to_result!(spdk_bdev_open_ext(
                name.as_ptr(),
                write,
                Some(handle_event),
                null_mut(),
                &mut desc))?;
        }

        Ok(Descriptor(NonNull::new(desc).unwrap()))
    }

    /// Returns a pointer to the underlying `spdk_bdev_desc` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev_desc {
        self.0.as_ptr()
    }

    /// Returns the [`Device`] associated with this [`Descriptor`].
    pub fn device(&self) -> Device<Any> {
        Device::from_ptr(unsafe { spdk_bdev_desc_get_bdev(self.0.as_ptr()) })
    }

    /// Returns an [`IoChannel`] for this [`Descriptor`].
    /// 
    /// I/O channels are bound to the `spdk_thread` on which this function is
    /// called. The returned [`IoChannel`] cannot be used from any other thread.
    pub fn io_channel(&self) -> Result<IoChannel<'_>, Errno> {
        IoChannel::new(self)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0.as_ptr()) }
    }
}
