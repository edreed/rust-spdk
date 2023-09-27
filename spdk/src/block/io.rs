//! Support for Storage Performance Development Kit block devices.
use std::{
    cell::RefCell,
    future::Future,
    os::raw::c_void,
    pin::Pin,
    ptr::{
        addr_of_mut,
        addr_of,

        null_mut,
    },
    rc::Rc,
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
    spdk_bdev_io,
    spdk_bdev_io_wait_entry,
    spdk_bdev_io_wait_entry__bindgen_ty_1,
    spdk_bdev_flush,
    spdk_bdev_free_io,
    spdk_bdev_queue_io_wait,
    spdk_bdev_read,
    spdk_bdev_reset,
    spdk_bdev_unmap,
    spdk_bdev_write,
    spdk_bdev_write_zeroes,
    spdk_bdev_zone_management,
};

use crate::{
    errors::{
        EIO,
        ENOMEM,
    },
    thread::Thread
};

use super::IoChannel;

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
pub(crate) struct Io<'a> {
    inner: Rc<RefCell<IoInner<'a>>>,
}

unsafe impl Send for Io<'_> {}

impl <'a> Io<'a> {
    /// Returns a new [`Io`] instance.
    fn new(channel: &'a IoChannel<'a>, op: IoOperation<'a>) -> Self {
        let inner = Rc::new(RefCell::new(IoInner::<'a> {
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

        inner.borrow_mut().wait.cb_arg =
            Rc::as_ptr(&inner).cast_mut() as *mut c_void;

        Io { inner }
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn complete(
        bdev_io: *mut spdk_bdev_io,
        success: bool,
        ctx: *mut c_void
    ) {
        let inner = Rc::from_raw(ctx as *mut RefCell<IoInner<'_>>);

        unsafe { spdk_bdev_free_io(bdev_io) };

        let waker = match inner.try_borrow_mut() {
            Ok(mut inner) => inner.set_result(success),
            Err(_) => {
                let inner = inner.clone();

                Thread::current().send_msg(move || {
                    let waker = inner.borrow_mut().set_result(success);

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
        let inner = Rc::from_raw((*wait).cb_arg as *mut RefCell<IoInner<'_>>);
        let waker = inner.borrow_mut().waker.take();

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
        let mut inner = self.inner.borrow_mut();

        if let Some(r) = inner.result.take() {
            return Poll::Ready(r);
        }

        inner.waker = Some(cx.waker().clone());

        let inner_raw = Rc::as_ptr(&self.inner).cast_mut();

        unsafe { Rc::increment_strong_count(inner_raw); }

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
            unsafe { Rc::decrement_strong_count(inner_raw); }

            return Poll::Ready(Err(e));
        }

        Poll::Pending
    }

    /// Resets the block device zone.
    pub(crate) fn reset_zone(channel: &'a IoChannel<'a>, zone_id: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::ManageZone { zone_id, action: SPDK_BDEV_ZONE_RESET })
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    pub(crate) fn write(channel: &'a IoChannel<'a>, buf: &'a [u8], offset: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Write { buf, offset })
    }

    /// Writes zeroes to the block device at the specified offset.
    pub(crate) fn write_zeroes(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::WriteZeroes { offset, len })
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    pub(crate) fn read(channel: &'a IoChannel<'a>, buf: &'a mut [u8], offset: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Read { buf, offset })
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub(crate) fn unmap(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Unmap { offset, len })
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub(crate) fn flush(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        Io::<'a>::new(channel, IoOperation::Flush { offset, len })
    }

    /// Resets the block device.
    pub(crate) fn reset(channel: &'a IoChannel<'a>) -> Self {
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
                            let inner_raw = Rc::as_ptr(&self.inner).cast_mut();

                            Rc::increment_strong_count(inner_raw);

                            let mut inner = self.inner.borrow_mut();

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

