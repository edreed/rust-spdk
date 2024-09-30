use std::{
    ffi::{
        c_int,
        c_void,
        CStr,
    },
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
    spdk_bdev_open_async,
};
use ternary_rs::if_else;

use crate::{
    block::Any,
    errors::{
        Errno,

        EINVAL,
        ENOMEM,
    },
    task::{
        Promise,
        Promissory,
    },

    to_poll_pending_on_ok,
};

use super::{
    Device,
    IoChannel,
};

/// A handle to an open block device.
#[derive(Debug)]
pub struct Descriptor(NonNull<spdk_bdev_desc>);

unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

impl Descriptor {
    unsafe extern "C" fn handle_event(_type: u32, _bdev: *mut spdk_bdev, _ctx: *mut c_void) {
    }

    unsafe extern "C" fn open_complete(desc: *mut spdk_bdev_desc, status: c_int, ctx: *mut c_void) {
        let p = Promissory::<Descriptor>::from_raw(ctx.cast());
        let res = if_else!(
            status == 0,
            Descriptor::try_from(desc).map_err(|_| EINVAL),
            Err(Errno(-status))
        );

        Promissory::set_result(p, res);
    }

    /// Open a block device by its name.
    pub async fn open(name: &CStr, write: bool) -> Result<Descriptor, Errno> {
        Promise::new(|p| {
            let (cb_fn, cb_arg) = (Self::open_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok!{
                unsafe {
                    spdk_bdev_open_async(
                        name.as_ptr(),
                        write,
                        Some(Self::handle_event),
                        null_mut(),
                        null_mut(),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe {drop(Promissory::from_raw(cb_arg)) };
                }
            }
        }).await
    }

    /// Returns a pointer to the underlying `spdk_bdev_desc` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev_desc {
        self.0.as_ptr()
    }

    /// Returns the [`Device`] associated with this [`Descriptor`].
    pub fn device(&self) -> Device<Any> {
        Device::<Any>::from_ptr(unsafe { spdk_bdev_desc_get_bdev(self.0.as_ptr()) })
    }

    /// Returns an [`IoChannel`] for this [`Descriptor`].
    /// 
    /// I/O channels are bound to the `spdk_thread` on which this function is
    /// called. The returned [`IoChannel`] cannot be used from any other thread.
    pub fn io_channel(&self) -> Result<IoChannel, Errno> {
        IoChannel::new(self)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0.as_ptr()) }
    }
}

impl TryFrom<*mut spdk_bdev_desc> for Descriptor {
    type Error = Errno;

    fn try_from(desc: *mut spdk_bdev_desc) -> Result<Self, Self::Error> {
        match NonNull::new(desc as *mut _) {
            Some(ptr) => Ok(Self(ptr)),
            None => Err(ENOMEM),
        }
    }
}
