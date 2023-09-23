//! Support for Storage Performance Development Kit block devices.
use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    ffi::CStr,
    future::Future,
    os::raw::c_void,
    pin::Pin,
    ptr::{
        addr_of_mut,
        addr_of,
        
        null_mut,
    },
    sync::{
        Arc,
        Mutex,
    },
    task::{
        Context,
        Poll,
        Waker
    },
};

use spdk_sys::{
    to_result, 

    SPDK_BDEV_ZONE_RESET,

    Errno,
    spdk_bdev,
    spdk_bdev_desc,
    spdk_bdev_io,
    spdk_bdev_io_wait_entry,
    spdk_bdev_io_wait_entry__bindgen_ty_1,
    spdk_io_channel,

    spdk_bdev_close,
    spdk_bdev_desc_get_bdev,
    spdk_bdev_first,
    spdk_bdev_flush,
    spdk_bdev_free_io,
    spdk_bdev_get_block_size,
    spdk_bdev_get_buf_align,
    spdk_bdev_get_by_name,
    spdk_bdev_get_io_channel,
    spdk_bdev_get_name,
    spdk_bdev_get_num_blocks,
    spdk_bdev_get_optimal_io_boundary,
    spdk_bdev_get_physical_block_size,
    spdk_bdev_get_product_name,
    spdk_bdev_get_write_unit_size,
    spdk_bdev_has_write_cache,
    spdk_bdev_is_zoned,
    spdk_bdev_next,
    spdk_bdev_open_ext,
    spdk_bdev_queue_io_wait,
    spdk_bdev_read,
    spdk_bdev_reset,
    spdk_bdev_unmap,
    spdk_bdev_write,
    spdk_bdev_write_zeroes,
    spdk_bdev_zone_management,
    spdk_put_io_channel,
};

use crate::{
    errors::{
        EBADF,
        EIO,
        ENOMEM,
    },
    thread::Thread
};

/// Represents a block device.
pub struct Device(*mut spdk_bdev);

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    /// Get a [`Device`] by its name.
    /// 
    /// # Returns
    /// 
    /// This function returns [`None`] if no block device with the given name
    /// exists.
    pub fn from_name(name: &CStr) -> Option<Self> {
        let bdev = unsafe { spdk_bdev_get_by_name(name.as_ptr()) };

        if bdev.is_null() {
            None
        } else {
            Some(Device(bdev))
        }
    }

    /// Get a [`Device`] for an `spdk_bdev`.
    #[cfg(feature = "bdev")]
    pub(crate) fn from_bdev(bdev: *mut spdk_bdev) -> Self {
        Self(bdev)
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev {
        self.0
    }

    /// Get the name of this block device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_name(self.0)) }
    }

    /// Get the product name of this block device.
    pub fn product_name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_product_name(self.0)) }
    }

    /// Get the logical block size of this block device in bytes.
    pub fn logical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_block_size(self.0) }
    }

    /// Get the number of logical blocks of this block device.
    pub fn logical_block_count(&self) -> u64 {
        unsafe { spdk_bdev_get_num_blocks(self.0) }
    }

    /// Get the physical block size of this block device in bytes.
    pub fn physical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_physical_block_size(self.0) }
    }

    /// Get the write unit size of this block device in logical blocks.
    ///
    /// This is the minimum number of blocks that can be written in a single
    /// operation. Write operations must be a multiple of the write unit size.
    pub fn write_unit_size(&self) -> u32 {
        unsafe { spdk_bdev_get_write_unit_size(self.0) }
    }

    /// Get the optimal I/O boundary of this block device in logical blocks.
    /// 
    /// This is the optimal boundary in logical blocks that should not be
    /// crosseed for best performance. This function returns `0` if there is
    /// no optimal I/O boundary.
    pub fn optimal_io_boundary(&self) -> u32 {
        unsafe { spdk_bdev_get_optimal_io_boundary(self.0) }
    }

    /// Get the minimum I/O buffer alignment, in bytes, of this block device.
    pub fn buffer_alignment(&self) -> usize {
        unsafe { spdk_bdev_get_buf_align(self.0) }
    }

    /// Get the [`Layout`] for a buffer of the specified byte size.
    pub fn layout_for_size(&self, size: usize) -> Result<Layout, LayoutError> {
        Layout::from_size_align(size, self.buffer_alignment())
    }

    /// Get the [`Layout`] for a buffer of the specified number of logical
    /// blocks.
    pub fn layout_for_blocks(&self, count: u64) -> Result<Layout, LayoutError> {
        self.layout_for_size(count as usize * self.logical_block_size() as usize)
    }
    
    /// Gets whether this block device supports zoned namespace semantics.
    pub fn is_zoned(&self) -> bool {
        unsafe { spdk_bdev_is_zoned(self.0) }
    }

    /// Gets whether this block device has an enabled write cache.
    pub fn has_write_cache(&self) -> bool {
        unsafe { spdk_bdev_has_write_cache(self.0) }
    }
}

/// An iterator over all block devices.
pub struct Devices(*mut spdk_bdev);

unsafe impl Send for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            None
        } else {
            let current = self.0;

            self.0 = unsafe { spdk_bdev_next(self.0) };

            Some(Device(current))
        }
    }
}

/// Get an iterator over all block devices.
pub fn devices() -> Devices {
    Devices(unsafe { spdk_bdev_first() })
}

/// Encapsulates a block device I/O operation and its parameters.
enum IoOperation<'a> {
    ManageZone {
        zone_id: u64,
        action: u32
    },
    Write {
        buf: &'a [u8],
        offset: u64,
    },
    WriteZeroes {
        offset: u64,
        len: u64,
    },
    Read {
        buf: &'a mut [u8],
        offset: u64,
    },
    Unmap {
        offset: u64,
        len: u64,
    },
    Flush {
        offset: u64,
        len: u64,
    },
    Reset,
}

/// Encapsulates the state of a block device I/O operation.
struct IoInner<'a> {
    op: IoOperation<'a>,
    channel: &'a IoChannel<'a>,
    result: Option<Result<(), Errno>>,
    waker: Option<Waker>,
    wait: spdk_bdev_io_wait_entry,
}

impl IoInner<'_> {
    /// Sets the result to the waiting future and returns a [`Waker`] to awaken
    /// the waiting future.
    fn set_result(&mut self, success: bool) -> Option<Waker> {
        if success {
            self.result = Some(Ok(()));
        } else {
            self.result = Some(Err(EIO));
        }

        self.waker.take()
    }
}

/// Represents the state of a block device I/O operation.
struct Io<'a> {
    inner: Arc<Mutex<IoInner<'a>>>,
}

unsafe impl Send for Io<'_> {}

impl <'a> Io<'a> {
    /// Returns a new [`Io`] instance.
    fn new(channel: &'a IoChannel<'a>, op: IoOperation<'a>) -> Self {
        let inner = Arc::new(Mutex::new(IoInner::<'a> {
            op,
            channel,
            result: None,
            waker: None,
            wait: spdk_bdev_io_wait_entry {
                bdev: channel.descriptor().device().as_ptr(),
                cb_fn: Some(Self::retry),
                cb_arg: null_mut(),
                link: spdk_bdev_io_wait_entry__bindgen_ty_1 {
                    tqe_next: null_mut(),
                    tqe_prev: null_mut(),
                },
            }
        }));

        inner.lock().unwrap().wait.cb_arg =
            Arc::as_ptr(&inner).cast_mut() as *mut c_void;

        Io { inner }
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn complete(
        bdev_io: *mut spdk_bdev_io,
        success: bool,
        ctx: *mut c_void
    ) {
        let inner = Arc::from_raw(ctx as *mut Mutex<IoInner<'_>>);

        unsafe { spdk_bdev_free_io(bdev_io) };

        let waker = match inner.try_lock() {
            Ok(mut inner) => inner.set_result(success),
            Err(_) => {
                let inner = inner.clone();

                Thread::current().send_msg(move || {
                    let waker = inner.lock().unwrap().set_result(success);

                    if let Some(w) = waker {
                        w.wake();
                    }
                }).expect("send result");
                None
            },
        };

        if let Some(w) = waker {
            w.wake();
        }
    }

    /// A callback invoked when a queued block device I/O operation can be started.
    unsafe extern "C" fn retry(ctx: *mut c_void) {
        let wait = ctx as *mut spdk_bdev_io_wait_entry;
        let inner = Arc::from_raw((*wait).cb_arg as *mut Mutex<IoInner<'_>>);
        let waker = inner.lock().unwrap().waker.take();

        if let Some(w) = waker {
            w.wake();
        }
    }

    /// Polls the block device I/O operation.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the I/O operation has completed successfully, this function returns
    /// [`Poll::Ready(Ok(()))`].
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    fn poll_io(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        let mut inner = self.inner.lock().unwrap();

        if let Some(r) = inner.result.take() {
            return Poll::Ready(r);
        }

        inner.waker = Some(cx.waker().clone());

        let inner_raw = Arc::as_ptr(&self.inner).cast_mut();

        unsafe { Arc::increment_strong_count(inner_raw); }

        let descriptor = inner.channel.descriptor().as_ptr();
        let channel = inner.channel.as_ptr();
        let res = match &mut inner.op {
            IoOperation::ManageZone { zone_id, action } => {
                unsafe {
                    to_result!(spdk_bdev_zone_management(
                        descriptor,
                        channel,
                        *zone_id,
                        *action,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
            IoOperation::Write { buf, offset } => {
                unsafe {
                    to_result!(spdk_bdev_write(
                        descriptor,
                        channel,
                        addr_of!(**buf) as *mut c_void,
                        *offset,
                        buf.len() as u64,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
            IoOperation::WriteZeroes { offset, len } => {
                unsafe {
                    to_result!(spdk_bdev_write_zeroes(
                        descriptor,
                        channel,
                        *offset,
                        *len,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            }
            IoOperation::Read { buf, offset } => {
                unsafe {
                    to_result!(spdk_bdev_read(
                        descriptor,
                        channel,
                        addr_of_mut!(**buf) as *mut c_void,
                        *offset,
                        buf.len() as u64,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
            IoOperation::Unmap { offset, len } => {
                unsafe {
                    to_result!(spdk_bdev_unmap(
                        descriptor,
                        channel,
                        *offset,
                        *len,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
            IoOperation::Flush { offset, len } => {
                unsafe {
                    to_result!(spdk_bdev_flush(
                        descriptor,
                        channel,
                        *offset,
                        *len,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
            IoOperation::Reset => {
                unsafe {
                    to_result!(spdk_bdev_reset(
                        descriptor,
                        channel,
                        Some(Self::complete),
                        inner_raw as *mut c_void))
                }
            },
        };

        if let Err(e) = res {
            unsafe { Arc::decrement_strong_count(inner_raw); }

            return Poll::Ready(Err(e));
        }

        Poll::Pending
    }

    /// Resets the block device zone.
    fn reset_zone(channel: &'a IoChannel<'a>, zone_id: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::ManageZone { zone_id, action: SPDK_BDEV_ZONE_RESET })
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    fn write(channel: &'a IoChannel<'a>, buf: &'a [u8], offset: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Write { buf, offset })
    }

    /// Writes zeroes to the block device at the specified offset.
    fn write_zeroes(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::WriteZeroes { offset, len })
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    fn read(channel: &'a IoChannel<'a>, buf: &'a mut [u8], offset: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Read { buf, offset })
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    fn unmap(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Unmap { offset, len })
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    fn flush(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Flush { offset, len })
    }

    /// Resets the block device.
    fn reset(channel: &'a IoChannel<'a>) -> Self {
        Io::<'a>::new(channel, IoOperation::Reset)
    }
}

impl <'a> Future for Io<'a> {
    type Output = Result<(), Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.poll_io(cx) {
            Poll::Pending => {
                Poll::Pending
            },
            Poll::Ready(r) => {
                match r {
                    Ok(()) => Poll::Ready(Ok(())),
                    Err(e) if e != ENOMEM => Poll::Ready(Err(e)),
                    _ => {
                        unsafe {
                            let inner_raw = Arc::as_ptr(&self.inner).cast_mut();

                            Arc::increment_strong_count(inner_raw);

                            let mut inner = self.inner.lock().unwrap();

                            to_result!(spdk_bdev_queue_io_wait(
                                inner.channel.descriptor().device().as_ptr(),
                                inner.channel.as_ptr(),
                                &mut inner.wait as *mut spdk_bdev_io_wait_entry))
                                .expect("wait queued")
                        };
                        Poll::Pending
                    },
                }
            },
        }
    }
}

/// A handle to a block device I/O channel.
pub struct IoChannel<'a> {
    desc: &'a Descriptor,
    channel: *mut spdk_io_channel,
}

unsafe impl Send for IoChannel<'_> {}
unsafe impl Sync for IoChannel<'_> {}

impl IoChannel<'_> {
    /// Returns the [`Descriptor`] associated with this [`IoChannel`].
    fn descriptor(&self) -> &Descriptor {
        self.desc
    }

    /// Returns a pointer to the underlying `spdk_io_channel` struct.
    pub fn as_ptr(&self) -> *mut spdk_io_channel {
        self.channel
    }

    /// Resets the block device zone.
    pub async fn reset_zone(&mut self, zone_id: u64) -> Result<(), Errno> {
        Io::reset_zone(&self, zone_id).await
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    pub async fn write_at<B: AsRef<[u8]>>(&mut self, buf: &B, offset: u64) -> Result<(), Errno> {
        Io::write(&self, buf.as_ref(), offset).await
    }

    /// Writes zeroes to the block device at the specified offset.
    pub async fn write_zeroes_at(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::write_zeroes(&self, offset, len).await
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    pub async fn read_at<B: AsMut<[u8]>>(&mut self, buf: &mut B, offset: u64) -> Result<(), Errno> {
        Io::read(&self, buf.as_mut(), offset).await
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub async fn unmap(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::unmap(&self, offset, len).await
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub async fn flush(&mut self, offset: u64, len: u64) -> Result<(), Errno> {
        Io::flush(&self, offset, len).await
    }

    /// Resets the block device.
    pub async fn reset(&mut self) -> Result<(), Errno> {
        Io::reset(&self).await
    }
}

impl Drop for IoChannel<'_> {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.channel) }
    }
}

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

        Ok(IoChannel {
            desc: self,
            channel,
        })
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.0) }
    }
}
