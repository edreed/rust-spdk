use std::{
    ffi::CStr,
    ptr::null_mut,
};

use spdk_sys::{
    to_result, 

    Errno,
    spdk_bdev,
    spdk_bdev_desc,
    spdk_bdev_close,
    spdk_bdev_desc_get_bdev,
    spdk_bdev_get_io_channel,
    spdk_bdev_open_ext,
};

use crate::errors::EBADF;

use super::{Device, IoChannel};

/// A handle to an open block device.
pub struct Descriptor(*mut spdk_bdev_desc);

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

        Ok(Descriptor(desc))
    }

    /// Returns a pointer to the underlying `spdk_bdev_desc` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev_desc {
        self.0
    }

    /// Returns the [`Device`] associated with this [`Descriptor`].
    pub fn device(&self) -> Device {
        Device(unsafe { spdk_bdev_desc_get_bdev(self.0) })
    }

    /// Returns an [`IoChannel`] for this [`Descriptor`].
    /// 
    /// I/O channels are bound to the `spdk_thread` on which this function is
    /// called. The returned [`IoChannel`] cannot be used from any other thread.
    pub fn io_channel(&self) -> Result<IoChannel<'_>, Errno> {
        let channel = unsafe { spdk_bdev_get_io_channel(self.0) };

        if channel.is_null() {
            return Err(EBADF);
        }

        Ok(IoChannel::new(self, channel))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0) }
    }
}
