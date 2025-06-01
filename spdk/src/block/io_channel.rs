use std::{
    fmt::{self, Debug, Formatter},
    io::{IoSlice, IoSliceMut},
    mem::zeroed,
    os::raw::c_void,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::Arc,
    task::Poll,
};

use spdk_sys::{
    iovec as IoVec, spdk_bdev, spdk_bdev_copy_blocks, spdk_bdev_desc, spdk_bdev_desc_get_bdev,
    spdk_bdev_flush, spdk_bdev_free_io, spdk_bdev_get_io_channel, spdk_bdev_io,
    spdk_bdev_io_wait_entry, spdk_bdev_queue_io_wait, spdk_bdev_read, spdk_bdev_read_blocks,
    spdk_bdev_readv, spdk_bdev_readv_blocks, spdk_bdev_reset, spdk_bdev_unmap,
    spdk_bdev_unmap_blocks, spdk_bdev_write, spdk_bdev_write_blocks, spdk_bdev_write_zeroes,
    spdk_bdev_write_zeroes_blocks, spdk_bdev_writev, spdk_bdev_writev_blocks,
    spdk_bdev_zone_management, spdk_io_channel, spdk_io_channel_get_thread, spdk_put_io_channel,
    SPDK_BDEV_ZONE_RESET,
};
use ternary_rs::if_else;

use crate::{
    errors::{Errno, EINVAL, EIO, ENOMEM},
    task::{Promise, Promissory},
    thread::Thread,
    to_poll_pending_on_ok,
};

use super::{Any, Descriptor, Device};

/// A handle to a block device I/O channel.
pub struct IoChannel {
    desc: NonNull<spdk_bdev_desc>,
    channel: NonNull<spdk_io_channel>,
}

impl IoChannel {
    /// Creates a new [`IoChannel`].
    pub(crate) fn new(desc: &Descriptor) -> Result<Self, Errno> {
        // SAFETY: `desc` is guaranteed to contain a non-null pointer. The SPDK
        // also guarantees the descriptor will live as long as there are
        // outstanding I/O channels.
        let desc = unsafe { NonNull::new_unchecked(desc.as_ptr()) };
        let channel = unsafe { spdk_bdev_get_io_channel(desc.as_ptr()) };

        match NonNull::new(channel) {
            Some(channel) => Ok(Self { desc, channel }),
            None => Err(ENOMEM),
        }
    }

    /// Returns the thread associated with this [`IoChannel`].
    pub fn thread(&self) -> Thread {
        // SAFETY: The thread associated with the I/O channel is guaranteed to
        // be non-null and valid.
        unsafe { Thread::from_ptr_unchecked(spdk_io_channel_get_thread(self.channel.as_ptr())) }
    }

    /// Returns the block device associated with this [`IoChannel`].
    pub fn device(&self) -> Device<Any> {
        // SAFETY: The descriptor associated with the I/O channel is guaranteed
        // to be non-null and valid.
        unsafe { Device::<Any>::from_ptr_unchecked(spdk_bdev_desc_get_bdev(self.desc.as_ptr())) }
    }

    /// Returns the raw [`spdk_bdev`] pointer associated with this [`IoChannel`].
    fn bdev(&self) -> *mut spdk_bdev {
        unsafe { spdk_bdev_desc_get_bdev(self.desc.as_ptr()) }
    }

    /// Returns the raw [`spdk_bdev_desc`] associated with this [`IoChannel`].
    fn descriptor(&self) -> *mut spdk_bdev_desc {
        self.desc.as_ptr()
    }

    /// Returns a pointer to the underlying [`spdk_io_channel`] struct.
    fn as_ptr(&self) -> *mut spdk_io_channel {
        self.channel.as_ptr()
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn io_complete(io: *mut spdk_bdev_io, success: bool, cx: *mut c_void) {
        unsafe {
            spdk_bdev_free_io(io);
        }

        let p = Promissory::<()>::from_raw(cx.cast());

        Promissory::set_result(p, if_else!(success, Ok(()), Err(EIO)));
    }

    /// Waits for an I/O to become available.
    ///
    /// When an I/O submission function returns `ENOMEM`, it means the I/O
    /// buffer pool has no available buffers on this thread. This function waits
    /// for an I/O buffer to become available.
    ///
    /// This function must only be called after one of the I/O submission
    /// functions returns `ENOMEM`.
    ///
    /// This function returns `Err(EINVAL)` if the I/O channel an I/O buffer is
    /// available on the current thread..
    async fn wait_io_available(&self) -> Result<(), Errno> {
        Promise::with_context(unsafe { zeroed::<spdk_bdev_io_wait_entry>() }, |p| {
            // SAFETY: The caller guarantees that there are no other
            // references to the `Promissory` instance, so we can safely get
            // a mutable reference to the user context. We convert the
            // mutable reference to a pointer so we can get the callback and
            // context from the `Arc<Promissory>` instance to store in the
            // context.
            let wait = addr_of_mut!(*Promissory::user_context_mut(p).expect("sole reference"));
            let (cb_fn, cb_arg) = Promissory::callback_with_ok(p);

            // SAFETY: The memory referenced by `wait` has been zeroed, so
            // overwriting its field is safe.
            unsafe {
                addr_of_mut!((*wait).bdev).write(self.bdev());
                addr_of_mut!((*wait).cb_fn).write(Some(cb_fn));
                addr_of_mut!((*wait).cb_arg).write(cb_arg.cast_mut() as *mut _);
            }

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_queue_io_wait(
                        self.bdev(),
                        self.channel.as_ptr(),
                        wait
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Executes an I/O operation, queuing the I/O for later execution if there
    /// are no `spdk_bdev_io` structures available.
    async fn execute_io<F>(&self, mut start_fn: F) -> Result<(), Errno>
    where
        F: FnMut(&mut Arc<Promissory<()>>) -> Poll<Result<(), Errno>>,
    {
        loop {
            match Promise::new(|p| (start_fn)(p)).await {
                Ok(()) => return Ok(()),
                Err(e) if e != ENOMEM => return Err(e),
                Err(_) => self.wait_io_available().await?,
            }
        }
    }

    /// Resets the block device zone.
    pub async fn reset_zone(&self, zone_id: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_zone_management(
                        self.descriptor(),
                        self.as_ptr(),
                        zone_id,
                        SPDK_BDEV_ZONE_RESET,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// byte offset.
    pub async fn write_at<B: AsRef<[u8]>>(&self, buf: &B, offset: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let buf = buf.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of!(*buf) as *mut c_void,
                        offset,
                        buf.len() as u64,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes the data in the slice of buffers to the block device at the specified
    /// byte offset.
    pub async fn write_vectored_at<'b, B>(
        &self,
        bufs: &'b B,
        offset: u64,
        length: u64,
    ) -> Result<(), Errno>
    where
        B: AsRef<[IoSlice<'b>]> + ?Sized,
    {
        self.execute_io(|p| {
            let bufs = bufs.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_writev(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of!(*bufs) as *mut IoVec,
                        bufs.len() as i32,
                        offset,
                        length,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// block offset.
    ///
    /// The buffer length must be a multiple of the block size of the device.
    pub async fn write_blocks_at<B: AsRef<[u8]>>(
        &self,
        buf: &B,
        offset_blocks: u64,
    ) -> Result<(), Errno> {
        self.execute_io(|p| {
            let buf = buf.as_ref();
            let logical_block_size = self.device().logical_block_size() as usize;

            if (buf.len() % logical_block_size) != 0 {
                return Poll::Ready(Err(EINVAL));
            }

            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of!(*buf) as *mut c_void,
                        offset_blocks,
                        (buf.len() / logical_block_size) as u64,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes the data in the slice of buffers to the block device at the specified
    /// block offset.
    pub async fn write_vectored_blocks_at<'b, B>(
        &self,
        bufs: &'b B,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> Result<(), Errno>
    where
        B: AsRef<[IoSlice<'b>]> + ?Sized,
    {
        self.execute_io(|p| {
            let bufs = bufs.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_writev_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of!(*bufs) as *mut IoVec,
                        bufs.len() as i32,
                        offset_blocks,
                        num_blocks,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes zeroes to the block device at the specified byte offset.
    pub async fn write_zeroes_at(&self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_zeroes(
                        self.descriptor(),
                        self.as_ptr(),
                        offset,
                        len,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Writes zeroes to the block device at the specified block offset.
    pub async fn write_zeroes_blocks_at(
        &self,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_zeroes_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        offset_blocks,
                        num_blocks,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Reads data from the block device at the specified byte offset into the
    /// buffer.
    pub async fn read_at<B: AsMut<[u8]>>(&self, buf: &mut B, offset: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_read(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of_mut!(*buf.as_mut()) as *mut c_void,
                        offset,
                        buf.as_mut().len() as u64,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Reads data from the block device at the specified byte offset into the
    /// slice of buffers.
    pub async fn read_vectored_at<'b, B>(
        &self,
        bufs: &'b mut B,
        offset: u64,
        length: u64,
    ) -> Result<(), Errno>
    where
        B: AsMut<[IoSliceMut<'b>]> + ?Sized,
    {
        self.execute_io(|p| {
            let bufs = bufs.as_mut();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_readv(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of_mut!(*bufs) as *mut IoVec,
                        bufs.len() as i32,
                        offset,
                        length,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Reads data from the block device at the specified block offset into the
    /// buffer.
    ///
    /// The buffer must be a multiple of the block size of the device.
    pub async fn read_blocks_at<B: AsMut<[u8]>>(
        &self,
        buf: &mut B,
        offset_blocks: u64,
    ) -> Result<(), Errno> {
        self.execute_io(|p| {
            let buf = buf.as_mut();
            let logical_block_size = self.device().logical_block_size() as usize;

            if (buf.len() % logical_block_size) != 0 {
                return Poll::Ready(Err(EINVAL));
            }

            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_read_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of_mut!(*buf) as *mut c_void,
                        offset_blocks,
                        (buf.len() / logical_block_size) as u64,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Reads data from the block device at the specified block offset into the
    /// slice of buffers.
    pub async fn read_vectored_blocks_at<'b, B>(
        &self,
        bufs: &'b mut B,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> Result<(), Errno>
    where
        B: AsMut<[IoSliceMut<'b>]> + ?Sized,
    {
        self.execute_io(|p| {
            let bufs = bufs.as_mut();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_readv_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        addr_of_mut!(*bufs) as *mut IoVec,
                        bufs.len() as i32,
                        offset_blocks,
                        num_blocks,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Copies blocks from the source block offset to the destination block offset.
    pub async fn copy_blocks(
        &self,
        src_offset_blocks: u64,
        dst_offset_blocks: u64,
        num_blocks: u64,
    ) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_copy_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        src_offset_blocks,
                        dst_offset_blocks,
                        num_blocks,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub async fn unmap(&self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_unmap(
                        self.descriptor(),
                        self.as_ptr(),
                        offset,
                        len,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Notifies the block device that the specified range of blocks is no longer
    /// valid.
    pub async fn unmap_blocks(&self, offset_blocks: u64, num_blocks: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_unmap_blocks(
                        self.descriptor(),
                        self.as_ptr(),
                        offset_blocks,
                        num_blocks,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    ///
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub async fn flush(&self, offset: u64, len: u64) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_flush(
                        self.descriptor(),
                        self.as_ptr(),
                        offset,
                        len,
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }

    /// Resets the block device.
    pub async fn reset(&self) -> Result<(), Errno> {
        self.execute_io(|p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_reset(
                        self.descriptor(),
                        self.as_ptr(),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
        })
        .await
    }
}

impl Drop for IoChannel {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.channel.as_ptr()) }
    }
}

impl Debug for IoChannel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IoChannel {{ bdev: {:?}, thread: {:?} }}",
            self.device(),
            self.thread()
        )
    }
}
