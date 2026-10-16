use std::{
    ffi::{CStr, c_void},
    mem::transmute,
    ptr::{NonNull, null_mut},
};

use spdk_sys::{
    spdk_bdev, spdk_bdev_close, spdk_bdev_desc, spdk_bdev_desc_get_bdev, spdk_bdev_get_io_channel,
    spdk_bdev_open_ext,
};

use crate::{
    Result,
    errors::{ENOMEM, Errno},
    to_result,
};

use super::{Device, IoChannel, OwnedBy};

/// Callback for handling descriptor events.
extern "C" fn handle_desc_event(_type: u32, _bdev: *mut spdk_bdev, _ctx: *mut c_void) {}

/// Open a block device by its name.
pub(crate) fn open_desc(name: &CStr, write: bool) -> Result<NonNull<spdk_bdev_desc>> {
    let mut raw_desc = null_mut();

    to_result! {
        unsafe {
            spdk_bdev_open_ext(
                name.as_ptr(),
                write,
                Some(handle_desc_event),
                null_mut(),
                &mut raw_desc as *mut *mut _,
            )
        }
    }?;

    // SAFETY: If `spdk_bdev_open_ext` succeeds, `raw_desc` is a non-null and valid `spdk_bdev_desc`
    // pointer.
    Ok(unsafe { NonNull::new_unchecked(raw_desc) })
}

/// A handle to an open block device.
///
/// # Note
///
/// While it is safe to share a reference to a [`Descriptor`] across threads, the underlying
/// `spdk_bdev_desc` must be closed on the same `spdk_thread` on which it was opened. It is
/// therefore not marked as `Send`, though it is `Sync`.
#[derive(Debug)]
#[repr(transparent)]
pub struct Descriptor(NonNull<spdk_bdev_desc>);

unsafe impl Sync for Descriptor {}

impl Descriptor {
    /// Open a block device by its name.
    pub fn open(name: &CStr, write: bool) -> Result<Self> {
        let desc = open_desc(name, write)?;

        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_bdev_desc>`.
        Ok(unsafe { transmute::<NonNull<spdk_bdev_desc>, Self>(desc) })
    }

    /// Returns a pointer to the underlying `spdk_bdev_desc` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev_desc {
        self.0.as_ptr()
    }

    /// Returns the [`Device`] associated with this [`Descriptor`].
    pub fn device(&self) -> Device<OwnedBy<'_, Self>> {
        unsafe { Device::with_owner(self, spdk_bdev_desc_get_bdev(self.0.as_ptr())) }
    }

    /// Returns an [`IoChannel`] for this [`Descriptor`].
    ///
    /// I/O channels are bound to the `spdk_thread` on which this function is called. The returned
    /// [`IoChannel`] cannot be used from any other thread.
    pub fn io_channel(&self) -> Result<IoChannel> {
        Ok(IoChannel(unsafe {
            NonNull::new(spdk_bdev_get_io_channel(self.0.as_ptr())).ok_or(ENOMEM)?
        }))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0.as_ptr()) }
    }
}

impl TryFrom<*mut spdk_bdev_desc> for Descriptor {
    type Error = Errno;

    fn try_from(desc: *mut spdk_bdev_desc) -> Result<Self> {
        match NonNull::new(desc as *mut _) {
            Some(ptr) => Ok(Self(ptr)),
            None => Err(ENOMEM),
        }
    }
}
