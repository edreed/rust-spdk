#![cfg_attr(docsrs, doc(cfg(feature = "bdev-module")))]

use std::{
    io::{IoSlice, IoSliceMut},
    marker::PhantomData,
    mem::transmute,
    os::raw::c_int,
    ptr::{self, NonNull},
    rc::{Rc, Weak},
    slice,
    task::Poll,
};

#[cfg(feature = "nvmf")]
use spdk_sys::spdk_bdev_io_set_nvme_status;
use spdk_sys::{
    spdk_bdev, spdk_bdev_io, spdk_bdev_io_complete, spdk_bdev_io_get_buf, spdk_bdev_io_get_iovec,
    spdk_bdev_io_get_thread, spdk_bdev_io_set_aio_status,
    spdk_bdev_io_status::{
        SPDK_BDEV_IO_STATUS_ABORTED, SPDK_BDEV_IO_STATUS_FAILED,
        SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED, SPDK_BDEV_IO_STATUS_MISCOMPARE,
        SPDK_BDEV_IO_STATUS_NOMEM, SPDK_BDEV_IO_STATUS_SUCCESS,
    },
    spdk_io_channel,
};

#[cfg(feature = "scsi")]
use spdk_sys::spdk_bdev_io_set_scsi_status;
use ternary_rs::if_else;

use crate::{
    bdev::{self, BDev, IoError, IoResult, IoType},
    errors::EINVAL,
    thread::{self, Promise, Promissory, Thread},
};

/// A type alias for the promissory that receives the buffer allocation from a call to the
/// [`IoRequest<T>::allocate_buffers`] method.
type AllocateBuffersPromissory<'a, T> = Promissory<(), IoError, PhantomData<&'a mut IoRequest<T>>>;

/// Represents driver-specific context for an I/O request.
///
/// The type parameter `T` is the I/O context type for the BDev implementation.
#[derive(Default)]
pub(super) struct IoRequestCtx<'a, T>
where
    T: Default,
{
    buf_promissory: Option<Weak<AllocateBuffersPromissory<'a, T>>>,
    inner: T,
}

/// A BDev I/O request.
///
/// The type parameter `T` is the I/O context type for the BDev implementation.
#[repr(transparent)]
pub struct IoRequest<T>
where
    T: Default,
{
    io: NonNull<spdk_bdev_io>,
    _ctx: PhantomData<T>,
}

impl<T> IoRequest<T>
where
    T: Default,
{
    /// Initializes a newly submitted I/O request.
    ///
    /// # Safety
    ///
    /// This function must only be called from the I/O submission callback to initialize a newly
    /// submitted I/O request. It initializes the driver's I/O context to a default value.
    pub(super) unsafe fn new_in_place(io: *mut spdk_bdev_io) -> Self {
        unsafe {
            (*io)
                .driver_ctx
                .as_mut_ptr()
                .cast::<IoRequestCtx<T>>()
                .write(Default::default());

            // SAFETY: This type is a transparent wrapper around `NonNull<spdk_bdev_io>`.
            transmute(NonNull::new_unchecked(io))
        }
    }
    /// Returns the raw pointer to the I/O request.
    pub(crate) fn as_ptr(&self) -> *mut spdk_bdev_io {
        self.io.as_ptr()
    }

    /// Returns the raw pointer to the block device associated with the I/O request.
    pub(crate) fn bdev(&self) -> *mut spdk_bdev {
        unsafe { self.io.as_ref().bdev }
    }

    /// Returns the type of the I/O request.
    pub fn io_type(&self) -> IoType {
        (unsafe { self.io.as_ref().type_ }).into()
    }

    /// Returns the thread associated with the I/O request. The I/O request must be completed on
    /// this thread.
    pub fn thread(&self) -> Thread<thread::OwnedBy<'_, Self>> {
        // SAFETY: The thread associated with the I/O request is guaranteed to be non-null and
        // valid for the lifetime of the I/O request.
        unsafe { Thread::with_owner_from_raw(self, spdk_bdev_io_get_thread(self.as_ptr())) }
    }

    /// Returns the block device associated with the I/O request.
    pub fn device(&self) -> BDev<bdev::OwnedBy<'_, Self>> {
        // SAFETY: The block device associated with the I/O request is guaranteed to be non-null and
        // valid for the lifetime of the I/O request.
        unsafe { BDev::with_owner(self, self.io.as_ref().bdev) }
    }

    /// Returns the buffers associated with the I/O request.
    pub fn buffers(&self) -> &[IoSlice<'_>] {
        unsafe {
            let mut iovecs = ptr::null_mut();
            let mut iovec_count: c_int = 0;

            spdk_bdev_io_get_iovec(self.as_ptr(), &mut iovecs, &mut iovec_count);

            slice::from_raw_parts(iovecs as *const IoSlice<'_>, iovec_count as usize)
        }
    }

    /// Returns the mutable buffers associated with the I/O request.
    pub fn buffers_mut(&mut self) -> &mut [IoSliceMut<'_>] {
        unsafe {
            let mut iovecs = ptr::null_mut();
            let mut iovec_count: c_int = 0;

            spdk_bdev_io_get_iovec(self.as_ptr(), &mut iovecs, &mut iovec_count);

            slice::from_raw_parts_mut(iovecs as *mut IoSliceMut<'_>, iovec_count as usize)
        }
    }

    /// Returns the offset in blocks for the I/O request.
    pub fn offset_blocks(&self) -> u64 {
        unsafe { self.io.as_ref().u.bdev.offset_blocks }
    }

    /// Returns the length in blocks for the I/O request.
    pub fn num_blocks(&self) -> u64 {
        unsafe { self.io.as_ref().u.bdev.num_blocks }
    }

    /// Returns the source offset in blocks for a copy I/O request.
    pub fn copy_source_offset_blocks(&self) -> u64 {
        unsafe { self.io.as_ref().u.bdev.copy.src_offset_blocks }
    }

    /// Returns a reference to the internal context associated with the I/O request.
    fn internal_ctx(&self) -> &IoRequestCtx<'_, T> {
        unsafe { &*self.io.as_ref().driver_ctx.as_ptr().cast() }
    }

    /// Returns a mutable reference to the internal context associated with the I/O request.
    fn internal_ctx_mut(&mut self) -> &mut IoRequestCtx<'_, T> {
        unsafe { &mut *self.io.as_mut().driver_ctx.as_mut_ptr().cast() }
    }

    /// Returns a reference to the implementation-defined context associated with the I/O request.
    pub fn ctx(&self) -> &T {
        &self.internal_ctx().inner
    }

    /// Returns a mutable reference to the implementation-defined context associated with the I/O
    /// request.
    pub fn ctx_mut(&mut self) -> &mut T {
        &mut self.internal_ctx_mut().inner
    }

    /// Invoked when buffers requested by [`IoRequest<T>::allocate_buffers`] have been allocated for
    /// the I/O request.
    ///
    /// [`IoRequest<T>::allocate_buffers`]: method@IoRequest::allocate_buffers
    unsafe extern "C" fn buffers_allocated(
        _ch: *mut spdk_io_channel,
        io: *mut spdk_bdev_io,
        success: bool,
    ) {
        let mut io: Self = Self {
            io: unsafe { NonNull::new_unchecked(io) },
            _ctx: PhantomData,
        };
        let p = io
            .internal_ctx_mut()
            .buf_promissory
            .take()
            .expect("promissory present")
            .upgrade()
            .expect("promissory has strong references");

        Promissory::set_result(p, if_else!(success, Ok(()), Err(EINVAL.into())));
    }

    /// Allocates buffers aligned to the BDev's requirement for the I/O request.
    ///
    /// Allocation will only occur if no buffers are assigned or the buffers are not aligned to the
    /// BDev's requirement. If the buffers are not aligned, this call will cause a copy from the
    /// current buffers to a bounce buffer on write or a copy from the bounce buffer to the current
    /// buffers on read.
    ///
    /// If no buffers are currently assigned to this I/O request, the `length` parameter specifies
    /// the size of the buffers to allocate in bytes. This value must be no larger than
    /// `SPDK_BDEV_LARGE_BUF_MAX_SIZE`.
    ///
    /// Any buffers allocated by this method will automatically be freed on completion of this I/O
    /// request.
    pub async fn allocate_buffers<'a>(&'a mut self, length: u64) -> IoResult<()> {
        Promise::with_context(PhantomData::<&'a mut Self>)
            .request(move |p| {
                self.internal_ctx_mut()
                    .buf_promissory
                    .replace(Rc::downgrade(p));

                unsafe {
                    spdk_bdev_io_get_buf(self.as_ptr(), Some(Self::buffers_allocated), length)
                };

                Poll::Pending
            })
            .await
    }

    /// Completes the I/O request with the specified status.
    ///
    /// # Safety
    ///
    /// This method may only be called on the thread used to submit the I/O operation.
    ///
    /// # Panics
    ///
    /// This method panics on debug builds if not called on the thread associated with the I/O.
    pub(super) unsafe fn complete(mut self, status: IoResult<()>) {
        debug_assert!(self.thread().is_current());

        let io_status = match status {
            Ok(_) => SPDK_BDEV_IO_STATUS_SUCCESS,
            Err(e) => match e {
                IoError::GeneralError(errno) => unsafe {
                    spdk_bdev_io_set_aio_status(self.as_ptr(), -errno.errno())
                },
                IoError::Aborted => SPDK_BDEV_IO_STATUS_ABORTED,
                IoError::FirstFusedFailed => SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED,
                IoError::Miscompare => SPDK_BDEV_IO_STATUS_MISCOMPARE,
                IoError::NoMem => SPDK_BDEV_IO_STATUS_NOMEM,
                #[cfg(feature = "scsi")]
                IoError::ScsiError(status) => unsafe {
                    spdk_bdev_io_set_scsi_status(
                        self.as_ptr(),
                        status.sc.into(),
                        status.sk.into(),
                        status.asc,
                        status.ascq,
                    )
                },
                #[cfg(feature = "nvmf")]
                IoError::NvmeError(status) => unsafe {
                    spdk_bdev_io_set_nvme_status(
                        self.as_ptr(),
                        status.cdw0,
                        status.sct.into(),
                        status.sc.into(),
                    )
                },
                IoError::Pending => panic!("I/O completed with pending status"),
                _ => SPDK_BDEV_IO_STATUS_FAILED,
            },
        };

        unsafe {
            ptr::drop_in_place(self.io.as_mut().driver_ctx.as_mut_ptr() as *mut IoRequestCtx<T>);

            spdk_bdev_io_complete(self.as_ptr(), io_status);
        }
    }
}
