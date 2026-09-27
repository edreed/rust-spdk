use std::{
    ffi::{CStr, c_int, c_void},
    ptr::{NonNull, null_mut},
};

use spdk_sys::{
    spdk_bdev, spdk_bdev_close, spdk_bdev_desc, spdk_bdev_desc_get_bdev, spdk_bdev_open_async,
};
use ternary_rs::if_else;

use crate::{
    Result,
    errors::{EINVAL, ENOMEM, Errno},
    task::{Promise, Promissory},
    to_poll_pending_on_ok,
};

use super::{Device, IoChannel, OwnedBy};

/// Callback for handling descriptor events.
extern "C" fn handle_desc_event(_type: u32, _bdev: *mut spdk_bdev, _ctx: *mut c_void) {}

/// Callback for when an asynchronous open operation completes.
///
/// # Safety
///
/// This function is called by the SPDK library and must adhere to the expected calling convention
/// and argument types. The `ctx` pointer must be a valid pointer to a `Promissory` object created
/// by `Promissory::into_raw`.
extern "C" fn open_desc_complete(desc: *mut spdk_bdev_desc, status: c_int, ctx: *mut c_void) {
    let p = unsafe { Promissory::<NonNull<spdk_bdev_desc>, Errno>::from_raw(ctx.cast()) };
    let res = if_else!(
        status == 0,
        NonNull::new(desc).ok_or(EINVAL),
        Err(Errno::new(-status))
    );

    Promissory::set_result(p, res);
}

/// Open a block device by its name.
pub(crate) async fn open_desc(name: &CStr, write: bool) -> Result<NonNull<spdk_bdev_desc>> {
    Promise::new()
        .request(|p| {
            let (cb_fn, cb_arg) = (open_desc_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_open_async(
                        name.as_ptr(),
                        write,
                        Some(handle_desc_event),
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
        })
        .await
}

/// A handle to an open block device.
///
/// # Note
///
/// While it is safe to share a reference to a [`Descriptor`] across threads, the underlying
/// `spdk_bdev_desc` must be closed on the same `spdk_thread` on which it was opened. It is
/// therefore not marked as `Send`, though it is `Sync`.
#[derive(Debug)]
pub struct Descriptor(NonNull<spdk_bdev_desc>);

unsafe impl Sync for Descriptor {}

impl Descriptor {
    /// Open a block device by its name.
    pub async fn open(name: &CStr, write: bool) -> Result<Descriptor> {
        open_desc(name, write).await.map(Descriptor)
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

    fn try_from(desc: *mut spdk_bdev_desc) -> Result<Self> {
        match NonNull::new(desc as *mut _) {
            Some(ptr) => Ok(Self(ptr)),
            None => Err(ENOMEM),
        }
    }
}
