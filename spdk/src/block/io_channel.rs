use std::{
    fmt::{self, Debug, Formatter},
    io::{IoSlice, IoSliceMut},
    marker::PhantomData,
    mem,
    os::raw::c_void,
    ptr::{NonNull, addr_of, addr_of_mut},
    rc::{Rc, Weak},
    task::Poll,
};

use spdk_sys::{
    SPDK_BDEV_ZONE_RESET, iovec as IoVec, spdk_bdev, spdk_bdev_copy_blocks, spdk_bdev_desc,
    spdk_bdev_desc_get_bdev, spdk_bdev_flush, spdk_bdev_free_io, spdk_bdev_get_io_channel,
    spdk_bdev_io, spdk_bdev_io_get_aio_status, spdk_bdev_io_get_status, spdk_bdev_io_status::*,
    spdk_bdev_io_wait_entry, spdk_bdev_queue_io_wait, spdk_bdev_read, spdk_bdev_read_blocks,
    spdk_bdev_readv, spdk_bdev_readv_blocks, spdk_bdev_reset, spdk_bdev_unmap,
    spdk_bdev_unmap_blocks, spdk_bdev_write, spdk_bdev_write_blocks, spdk_bdev_write_zeroes,
    spdk_bdev_write_zeroes_blocks, spdk_bdev_writev, spdk_bdev_writev_blocks,
    spdk_bdev_zone_management, spdk_io_channel, spdk_io_channel_get_thread, spdk_put_io_channel,
};

use crate::{
    Result,
    block::IoResult,
    errors::{EINVAL, ENOMEM, Errno},
    task::{Promise, Promissory},
    thread::Thread,
    to_poll_pending_on_ok,
};

use super::{Any, Descriptor, Device, IoError};

#[cfg(feature = "nvmf")]
use crate::nvme::NvmeStatus;
#[cfg(feature = "nvmf")]
use spdk_sys::spdk_bdev_io_get_nvme_status;

#[cfg(feature = "scsi")]
use crate::scsi::ScsiStatus;
#[cfg(feature = "scsi")]
use spdk_sys::spdk_bdev_io_get_scsi_status;

type IoWaitPromissory<'a> = Promissory<(), IoError, IoWait<'a>>;

/// A wrapper around [`spdk_bdev_io_wait_entry`] that manages ownership of a weak pointer to the
/// [`Promissory`] instance awaiting availability of an [`spdk_bdev_io`] structure.
struct IoWait<'a>(spdk_bdev_io_wait_entry, PhantomData<&'a mut IoChannel>);

impl<'a> IoWait<'a> {
    /// Creates a new instance of an `IoWait` structure.
    fn new(p: &Weak<IoWaitPromissory>, channel: &'a mut IoChannel) -> Self {
        let mut wait: spdk_bdev_io_wait_entry = unsafe { mem::zeroed() };

        wait.bdev = channel.bdev();
        wait.cb_fn = Some(IoChannel::wait_io_complete);
        wait.cb_arg = p.clone().into_raw() as *mut _;

        Self(wait, PhantomData)
    }
}

impl Drop for IoWait<'_> {
    fn drop(&mut self) {
        drop(unsafe { Rc::from_raw(self.0.cb_arg as *const IoWaitPromissory) });
    }
}

type IoPromissory<T> = Promissory<(), IoError, PhantomData<T>>;

/// A handle to a block device I/O channel.
pub struct IoChannel {
    desc: NonNull<spdk_bdev_desc>,
    channel: NonNull<spdk_io_channel>,
}

impl IoChannel {
    /// Creates a new [`IoChannel`].
    pub(crate) fn new(desc: &Descriptor) -> Result<Self> {
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

    /// A callback invoked when an [`spdk_bdev_io`] structure is available to satisfy the request
    /// queued by [`spdk_bdev_queue_io_wait`].
    unsafe extern "C" fn wait_io_complete(ctx: *mut c_void) {
        let w = unsafe { Weak::from_raw(ctx as *const _ as *const IoWaitPromissory) };

        Promissory::set_result(
            w.upgrade().expect("promissory has strong references"),
            Ok(()),
        );
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
    async fn wait_io_available(&mut self) -> IoResult<()> {
        let ch = self.channel;

        Promise::with_context_cyclic(|w| IoWait::new(w, self))
            .request(|p| {
                let wait: &IoWait = Promissory::user_context(p);

                to_poll_pending_on_ok! {
                    unsafe {
                        spdk_bdev_queue_io_wait(
                            wait.0.bdev,
                            ch.as_ptr(),
                            &wait.0 as *const _ as *mut _
                        )
                    }
                }
                .map_err(Into::into)
            })
            .await
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn io_complete(io: *mut spdk_bdev_io, success: bool, cx: *mut c_void) {
        let p = unsafe { Promissory::<(), IoError>::from_raw(cx.cast()) };
        let res = if success {
            Ok(())
        } else {
            let io_error = match unsafe { spdk_bdev_io_get_status(io) } {
                SPDK_BDEV_IO_STATUS_AIO_ERROR => {
                    let mut aio_result: i32 = 0;

                    unsafe {
                        spdk_bdev_io_get_aio_status(io, &mut aio_result);
                    }

                    IoError::GeneralError(Errno::new(-aio_result))
                }
                SPDK_BDEV_IO_STATUS_ABORTED => IoError::Aborted,
                SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED => IoError::FirstFusedFailed,
                SPDK_BDEV_IO_STATUS_MISCOMPARE => IoError::Miscompare,
                SPDK_BDEV_IO_STATUS_NOMEM => IoError::NoMem,
                #[cfg(feature = "scsi")]
                SPDK_BDEV_IO_STATUS_SCSI_ERROR => {
                    let mut sc: i32 = 0;
                    let mut sk: i32 = 0;
                    let mut asc: i32 = 0;
                    let mut ascq: i32 = 0;

                    unsafe {
                        spdk_bdev_io_get_scsi_status(io, &mut sc, &mut sk, &mut asc, &mut ascq);
                    }

                    IoError::ScsiError(ScsiStatus {
                        sc: sc.try_into().expect("valid status code"),
                        sk: sk.try_into().expect("valid sense key"),
                        asc: asc.try_into().expect("valid additional sense code"),
                        ascq: ascq
                            .try_into()
                            .expect("valid additional sense code qualifier"),
                    })
                }
                #[cfg(feature = "nvmf")]
                SPDK_BDEV_IO_STATUS_NVME_ERROR => {
                    let mut cdw0: u32 = 0;
                    let mut sct: i32 = 0;
                    let mut sc: i32 = 0;

                    unsafe {
                        spdk_bdev_io_get_nvme_status(io, &mut cdw0, &mut sct, &mut sc);
                    }

                    IoError::NvmeError(NvmeStatus {
                        cdw0,
                        sct: sct.try_into().expect("valid status code type"),
                        sc: sc.try_into().expect("valid status code"),
                    })
                }
                SPDK_BDEV_IO_STATUS_PENDING => {
                    panic!("I/O completed with pending status");
                }
                _ => IoError::Failed,
            };

            Err(io_error)
        };

        unsafe {
            spdk_bdev_free_io(io);
        }

        Promissory::set_result(p, res);
    }

    /// Executes an I/O operation, queuing the I/O for later execution if there
    /// are no `spdk_bdev_io` structures available.
    async fn execute_io<F, T>(&mut self, data: PhantomData<T>, mut start_fn: F) -> IoResult<()>
    where
        F: FnMut(&mut Self, &mut Rc<IoPromissory<T>>) -> Poll<IoResult<()>>,
        T: Unpin,
    {
        loop {
            match Promise::with_context(data)
                .request(|p| (start_fn)(self, p))
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) if !matches!(e, IoError::NoMem) => return Err(e),
                Err(_) => self.wait_io_available().await?,
            }
        }
    }

    /// Resets the block device zone.
    pub async fn reset_zone<'a>(&'a mut self, zone_id: u64) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_zone_management(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// byte offset.
    pub async fn write_at<'a, B: AsRef<[u8]>>(
        &'a mut self,
        buf: &'a B,
        offset: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<(&'a mut Self, &'a B)>, |this, p| {
            let buf = buf.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes the data in the slice of buffers to the block device at the specified
    /// byte offset.
    pub async fn write_vectored_at<'a, B>(
        &'a mut self,
        bufs: &'a B,
        offset: u64,
        length: u64,
    ) -> IoResult<()>
    where
        B: AsRef<[IoSlice<'a>]> + ?Sized,
    {
        self.execute_io(PhantomData::<(&'a mut Self, &'a B)>, |this, p| {
            let bufs = bufs.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_writev(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// block offset.
    ///
    /// The buffer length must be a multiple of the block size of the device.
    pub async fn write_blocks_at<'a, B: AsRef<[u8]>>(
        &'a mut self,
        buf: &'a B,
        offset_blocks: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<(&'a mut Self, &'a B)>, |this, p| {
            let buf = buf.as_ref();
            let logical_block_size = this.device().logical_block_size() as usize;

            if (buf.len() % logical_block_size) != 0 {
                return Poll::Ready(Err(EINVAL.into()));
            }

            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes the data in the slice of buffers to the block device at the specified
    /// block offset.
    pub async fn write_vectored_blocks_at<'a, B>(
        &'a mut self,
        bufs: &'a B,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> IoResult<()>
    where
        B: AsRef<[IoSlice<'a>]> + ?Sized,
    {
        self.execute_io(PhantomData::<(&'a mut Self, &'a B)>, |this, p| {
            let bufs = bufs.as_ref();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_writev_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes zeroes to the block device at the specified byte offset.
    pub async fn write_zeroes_at<'a>(&'a mut self, offset: u64, len: u64) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_zeroes(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Writes zeroes to the block device at the specified block offset.
    pub async fn write_zeroes_blocks_at<'a>(
        &'a mut self,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_write_zeroes_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Reads data from the block device at the specified byte offset into the
    /// buffer.
    pub async fn read_at<'a, B: AsMut<[u8]>>(
        &'a mut self,
        buf: &'a mut B,
        offset: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<(&'a mut Self, &'a mut B)>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_read(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Reads data from the block device at the specified byte offset into the
    /// slice of buffers.
    pub async fn read_vectored_at<'a, B>(
        &'a mut self,
        bufs: &'a mut B,
        offset: u64,
        length: u64,
    ) -> IoResult<()>
    where
        B: AsMut<[IoSliceMut<'a>]> + ?Sized,
    {
        self.execute_io(PhantomData::<(&'a mut Self, &'a mut B)>, |this, p| {
            let bufs = bufs.as_mut();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_readv(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Reads data from the block device at the specified block offset into the
    /// buffer.
    ///
    /// The buffer must be a multiple of the block size of the device.
    pub async fn read_blocks_at<'a, B: AsMut<[u8]>>(
        &'a mut self,
        buf: &'a mut B,
        offset_blocks: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<(&'a mut Self, &'a mut B)>, |this, p| {
            let buf = buf.as_mut();
            let logical_block_size = this.device().logical_block_size() as usize;

            if (buf.len() % logical_block_size) != 0 {
                return Poll::Ready(Err(IoError::GeneralError(EINVAL)));
            }

            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_read_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Reads data from the block device at the specified block offset into the
    /// slice of buffers.
    pub async fn read_vectored_blocks_at<'a, B>(
        &'a mut self,
        bufs: &'a mut B,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> IoResult<()>
    where
        B: AsMut<[IoSliceMut<'a>]> + ?Sized,
    {
        self.execute_io(PhantomData::<(&'a mut Self, &'a mut B)>, |this, p| {
            let bufs = bufs.as_mut();
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_readv_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Copies blocks from the source block offset to the destination block offset.
    pub async fn copy_blocks<'a>(
        &'a mut self,
        src_offset_blocks: u64,
        dst_offset_blocks: u64,
        num_blocks: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_copy_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub async fn unmap<'a>(&'a mut self, offset: u64, len: u64) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_unmap(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Notifies the block device that the specified range of blocks is no longer
    /// valid.
    pub async fn unmap_blocks<'a>(
        &'a mut self,
        offset_blocks: u64,
        num_blocks: u64,
    ) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_unmap_blocks(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    ///
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub async fn flush<'a>(&'a mut self, offset: u64, len: u64) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_flush(
                        this.descriptor(),
                        this.as_ptr(),
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
            .map_err(Into::into)
        })
        .await
    }

    /// Resets the block device.
    pub async fn reset<'a>(&'a mut self) -> IoResult<()> {
        self.execute_io(PhantomData::<&'a mut Self>, |this, p| {
            let (cb_fn, cb_arg) = (Self::io_complete, Promissory::into_raw(p.clone()));

            to_poll_pending_on_ok! {
                unsafe {
                    spdk_bdev_reset(
                        this.descriptor(),
                        this.as_ptr(),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    )
                }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
            .map_err(Into::into)
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
