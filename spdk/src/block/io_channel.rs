use std::{
    mem::MaybeUninit,
    os::raw::c_void,
    ptr::{
        NonNull,

        addr_of,
        addr_of_mut,
    },
    task::Poll,
};

use spdk_sys::{
    spdk_bdev_io_wait_entry,
    spdk_bdev_io,
    spdk_io_channel,

    SPDK_BDEV_ZONE_RESET,

    spdk_bdev_flush,
    spdk_bdev_free_io,
    spdk_bdev_get_io_channel,
    spdk_bdev_queue_io_wait,
    spdk_bdev_read,
    spdk_bdev_reset,
    spdk_bdev_unmap,
    spdk_bdev_write_zeroes,
    spdk_bdev_write,
    spdk_bdev_zone_management,
    spdk_put_io_channel,
};
use ternary_rs::if_else;

use crate::{
    errors::{
        Errno,

        EIO,
        ENOMEM,
    },
    task::{
        Promise,
        
        complete_with_ok,
        complete_with_status,
    },
    to_poll_pending_on_ok,
};

use super::Descriptor;

/// A handle to a block device I/O channel.
pub struct IoChannel<'a> {
    desc: &'a Descriptor,
    channel: NonNull<spdk_io_channel>,
}

unsafe impl Send for IoChannel<'_> {}
unsafe impl Sync for IoChannel<'_> {}

impl <'a> IoChannel<'a> {
    /// Creates a new [`IoChannel`].
    pub(crate) fn new(desc: &'a Descriptor) -> Result<Self, Errno> {
        let channel = unsafe { spdk_bdev_get_io_channel(desc.as_ptr()) };
        
        match NonNull::new(channel) {
            Some(channel) => Ok(Self { desc, channel }),
            None => Err(ENOMEM),
        }
    }

    /// Returns the [`Descriptor`] associated with this [`IoChannel`].
    pub fn descriptor(&self) -> &Descriptor {
        self.desc
    }

    /// Returns a pointer to the underlying `spdk_io_channel` struct.
    pub fn as_ptr(&self) -> *mut spdk_io_channel {
        self.channel.as_ptr()
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn complete_io(
        io: *mut spdk_bdev_io,
        success: bool,
        cx: *mut c_void
    ) {
        unsafe { spdk_bdev_free_io(io); }

        let status: i32 = EIO.into();

        complete_with_status(cx, if_else!(success, 0, -status))
    }

    /// Executes an I/O operation, queuing the I/O for later execution if there
    /// are no `spdk_bdev_io` structures available.
    async fn execute_io<F>(&self, mut start_fn: F) -> Result<(), Errno>
    where
        F: FnMut(*mut c_void) -> Poll<Result<(), Errno>>,
    {
        loop {
            match Promise::new(|cx| (&mut start_fn)(cx)).await {
                Ok(()) => return Ok(()),
                Err(e) if e != ENOMEM => return Err(e),
                Err(_) => {
                    unsafe {
                        Promise::new(|cx| {
                            let mut wait: spdk_bdev_io_wait_entry = MaybeUninit::zeroed().assume_init();

                            wait.bdev = self.desc.device().as_ptr();
                            wait.cb_fn = Some(complete_with_ok);
                            wait.cb_arg = cx;

                            to_poll_pending_on_ok!(spdk_bdev_queue_io_wait(
                                wait.bdev,
                                self.channel.as_ptr(),
                                &wait as *const _ as *mut _))
                            }).await?
                    }
                }
            }
        }
    }

    /// Resets the block device zone.
    pub async fn reset_zone(&mut self, zone_id: u64) -> Result<(), Errno> {
        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_zone_management(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    zone_id,
                    SPDK_BDEV_ZONE_RESET,
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    pub async fn write_at<B: AsRef<[u8]>>(&mut self, buf: &B, offset: u64) -> Result<(), Errno> {
        self.execute_io(|cx| {
            let buf = buf.as_ref();

            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_write(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    addr_of!(*buf) as *mut c_void,
                    offset,
                    buf.len() as u64,
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Writes zeroes to the block device at the specified offset.
    pub async fn write_zeroes_at(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_write_zeroes(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    offset,
                    len,
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    pub async fn read_at<B: AsMut<[u8]>>(&mut self, buf: &mut B, offset: u64) -> Result<(), Errno> {

        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_read(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    addr_of_mut!(*buf.as_mut()) as *mut c_void,
                    offset,
                    buf.as_mut().len() as u64,
                        Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub async fn unmap(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_unmap(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    offset,
                    len,
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub async fn flush(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_flush(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    offset,
                    len,
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }

    /// Resets the block device.
    pub async fn reset(&mut self) -> Result<(), Errno> {
        self.execute_io(|cx| {
            unsafe {
                to_poll_pending_on_ok!(spdk_bdev_reset(
                    self.descriptor().as_ptr(),
                    self.as_ptr(),
                    Some(Self::complete_io),
                    cx))
            }
        }).await
    }
}

impl Drop for IoChannel<'_> {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.channel.as_ptr()) }
    }
}

