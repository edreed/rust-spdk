//! Support for Storage Performance Development Kit block devices.
use std::{
    cell::RefCell,
    future::Future,
    mem::ManuallyDrop,
    os::raw::c_void,
    pin::Pin,
    ptr::{
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

/// Encapsulates a block device I/O operation's parameters.
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
struct IoState {
    result: Option<Result<(), Errno>>,
    waker: Option<Waker>,
    wait: spdk_bdev_io_wait_entry,
}

impl IoState {
    /// Sets the result and returns a [`Waker`] to awaken the waiting future.
    fn set_result(&mut self, success: bool) -> Option<Waker> {
        if success {
            self.result = Some(Ok(()));
        } else {
            self.result = Some(Err(EIO));
        }

        self.waker.take()
    }
}

/// A function that starts a block device I/O operation.
type StartIoFn = unsafe fn(data: *const (), cx: *mut Context<'_>) -> Poll<Result<(), Errno>>;

/// Orchestrates the execution of a block device I/O operation.
struct IoTask<'a> {
    channel: &'a IoChannel<'a>,
    start_io_fn: StartIoFn,
    op: IoOperation<'a>,
    state: RefCell<IoState>
}

impl <'a> IoTask<'a> {
    /// Returns a new [`IoTask`] instance.
    fn new(channel: &'a IoChannel<'a>, start_io_fn: StartIoFn, op: IoOperation<'a>) -> Rc::<Self> {
        Rc::new(IoTask::<'a> {
            channel,
            start_io_fn,
            op,
            state: RefCell::new(IoState{
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
            }),
        })
    }

    /// A callback invoked when a block device I/O operation completes.
    unsafe extern "C" fn complete(
        bdev_io: *mut spdk_bdev_io,
        success: bool,
        ctx: *mut c_void
    ) {
        let task = Rc::from_raw(ctx as *mut IoTask<'_>);

        unsafe { spdk_bdev_free_io(bdev_io) };

        let waker = match task.state.try_borrow_mut() {
            Ok(mut state) => state.set_result(success),
            Err(_) => {
                let task = task.clone();

                Thread::current().send_msg(move || {
                    let waker = task.state.borrow_mut().set_result(success);

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
        let task = Rc::from_raw((*wait).cb_arg as *mut IoTask<'_>);
        let waker = task.state.borrow_mut().waker.take();

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
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    fn poll_io(
        rc_self: &Rc<IoTask>,
        cx: &mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let mut state = rc_self.state.borrow_mut();

        if let Some(r) = state.result.take() {
            return Poll::Ready(r);
        }

        state.waker = Some(cx.waker().clone());

        let self_raw = Rc::as_ptr(rc_self);

        unsafe {
            Rc::increment_strong_count(self_raw);

            return match (rc_self.start_io_fn)(self_raw.cast(), cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) if e != ENOMEM => {
                    Rc::decrement_strong_count(self_raw);

                    Poll::Ready(Err(e))
                },
                Poll::Ready(Err(_)) => {
                    to_result!(spdk_bdev_queue_io_wait(
                        rc_self.channel.descriptor().device().as_ptr(),
                        rc_self.channel.as_ptr(),
                        &mut rc_self.state.borrow_mut().wait as *mut spdk_bdev_io_wait_entry))
                        .expect("wait queued");
                    Poll::Pending
                },
                Poll::Ready(Ok(())) => unreachable!(),
            }
        }
    }

    /// Starts a zone management operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_manage_zone(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::ManageZone { zone_id, action } = rc_io.op  {
            let res = to_result!(spdk_bdev_zone_management(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                zone_id,
                action,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts a write operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_write(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::Write { buf, offset } = rc_io.op  {
            let res = to_result!(spdk_bdev_write(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                addr_of!(*buf) as *mut c_void,
                offset,
                buf.len() as u64,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts a write zeroes operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_write_zeroes(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::WriteZeroes { offset, len } = rc_io.op  {
            let res = to_result!(spdk_bdev_write_zeroes(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                offset,
                len,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts a read operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_read(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::Read { buf, offset } = &rc_io.op  {
            let res = to_result!(spdk_bdev_read(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                addr_of!(**buf) as *mut c_void,
                *offset,
                buf.len() as u64,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts an unmap operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_unmap(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::Unmap { offset, len } = rc_io.op  {
            let res = to_result!(spdk_bdev_unmap(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                offset,
                len,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts a flush operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_flush(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::Flush { offset, len } = rc_io.op  {
            let res = to_result!(spdk_bdev_flush(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                offset,
                len,
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }

    /// Starts a reset operation on a block device.
    /// 
    /// # Returns
    /// 
    /// This function returns [`Poll::Pending`] if the I/O operation is started
    /// successfully.
    /// 
    /// If the operation cannot be started because there are no available
    /// `spdk_bdev_io` objects, this function returns
    /// `Poll::Ready(Err(ENOMEM))`.
    /// 
    /// If the operation cannot be started for any other reason, this function
    /// returns `Poll::Ready(Err(e))` where `e` is an [`Errno`] value.
    unsafe fn start_reset(
        data: *const (),
        _cx: *mut Context<'_>
    ) -> Poll<Result<(), Errno>> {
        let rc_io = ManuallyDrop::new(Rc::<IoTask<'_>>::from_raw(data.cast()));

        if let IoOperation::Reset = rc_io.op  {
            let res = to_result!(spdk_bdev_reset(
                rc_io.channel.descriptor().as_ptr(),
                rc_io.channel.as_ptr(),
                Some(Self::complete),
                Rc::as_ptr(&rc_io) as *mut c_void));

            return match res {
                Ok(()) => Poll::Pending,
                Err(e) => Poll::Ready(Err(e)),
            }
        }

        unreachable!("parameter mismatch");
    }
}

/// Represents the state of a block device I/O operation.
pub(crate) struct Io<'a> {
    task: Rc<IoTask<'a>>
}

unsafe impl Send for Io<'_> {}

impl <'a> Io<'a> {
    /// Resets the block device zone.
    pub(crate) fn reset_zone(channel: &'a IoChannel<'a>, zone_id: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_manage_zone,
            IoOperation::ManageZone { zone_id, action: SPDK_BDEV_ZONE_RESET });

        Self { task }
    }

    /// Writes the data in the buffer to the block device at the specified
    /// offset.
    pub(crate) fn write(channel: &'a IoChannel<'a>, buf: &'a [u8], offset: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_write,
            IoOperation::Write { buf, offset });

        Self { task }
    }

    /// Writes zeroes to the block device at the specified offset.
    pub(crate) fn write_zeroes(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_write_zeroes,
            IoOperation::WriteZeroes { offset, len });

        Self { task }
    }

    /// Reads data from the block device at the specified offset into the
    /// buffer.
    pub(crate) fn read(channel: &'a IoChannel<'a>, buf: &'a mut [u8], offset: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_read,
            IoOperation::Read { buf, offset });

        Self { task }
    }

    /// Notifies the block device that the specified range of bytes is no longer
    /// valid.
    pub(crate) fn unmap(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_unmap,
            IoOperation::Unmap { offset, len });

        Self { task }
    }

    /// Flushes the specified range of bytes from the volatile cache to the
    /// block device.
    /// 
    /// For devices with volatile cache, data is not guaranteed to be persistent
    /// until the completion of the flush operation.
    pub(crate) fn flush(channel: &'a IoChannel<'a>, offset: u64, len: u64) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_flush,
            IoOperation::Flush { offset, len });

        Self { task }
    }

    /// Resets the block device.
    pub(crate) fn reset(channel: &'a IoChannel<'a>) -> Self {
        let task = IoTask::new(
            channel,
            IoTask::start_reset,
            IoOperation::Reset);

        Self { task }
    }
}

impl <'a> Future for Io<'a> {
    type Output = Result<(), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        IoTask::poll_io(&self.task, cx)
    }
}

