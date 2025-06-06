use std::{
    ffi::{CStr, CString},
    io::{IoSlice, IoSliceMut},
    marker::PhantomData,
    mem::{self, offset_of, size_of, ManuallyDrop},
    os::raw::{c_int, c_void},
    ptr::{self, addr_of, addr_of_mut, drop_in_place, NonNull},
    slice,
    sync::Arc,
    task::Poll,
};

use async_trait::async_trait;
use spdk_sys::{
    spdk_bdev, spdk_bdev_destruct_done, spdk_bdev_fn_table, spdk_bdev_io, spdk_bdev_io_complete,
    spdk_bdev_io_get_buf, spdk_bdev_io_get_iovec, spdk_bdev_io_get_thread, spdk_bdev_io_status,
    spdk_bdev_io_type, spdk_bdev_module, spdk_bdev_register, spdk_bdev_unregister,
    spdk_get_io_channel, spdk_io_channel, spdk_io_channel_get_ctx, spdk_io_channel_get_thread,
    spdk_io_device_register, spdk_io_device_unregister, SPDK_BDEV_IO_STATUS_ABORTED,
    SPDK_BDEV_IO_STATUS_AIO_ERROR, SPDK_BDEV_IO_STATUS_FAILED,
    SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED, SPDK_BDEV_IO_STATUS_MISCOMPARE,
    SPDK_BDEV_IO_STATUS_NOMEM, SPDK_BDEV_IO_STATUS_NVME_ERROR, SPDK_BDEV_IO_STATUS_PENDING,
    SPDK_BDEV_IO_STATUS_SCSI_ERROR, SPDK_BDEV_IO_STATUS_SUCCESS,
};
use ternary_rs::if_else;

use crate::{
    block::{Any, Device, IoType, Owned, OwnedOps},
    errors::{Errno, ECANCELED, EINPROGRESS, EINVAL, ENOMEM},
    task::{Promise, Promissory},
    thread::{self, Thread},
    to_result,
};

/// The status of an I/O operation.
///
/// # Notes
///
/// These are mapped directly to the corresponding [`spdk_bdev_io_status`] values.
#[derive(Copy, Clone)]
pub enum IoStatus {
    AioError = -8,
    Aborted = -7,
    FirstFusedFailed = -6,
    Miscompare = -5,
    NoMem = -4,
    ScsiError = -3,
    NvmeError = -2,
    Failed = -1,
    Pending = 0,
    Success = 1,
}

impl From<spdk_bdev_io_status> for IoStatus {
    fn from(value: spdk_bdev_io_status) -> Self {
        match value {
            SPDK_BDEV_IO_STATUS_AIO_ERROR => IoStatus::AioError,
            SPDK_BDEV_IO_STATUS_ABORTED => IoStatus::Aborted,
            SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED => IoStatus::FirstFusedFailed,
            SPDK_BDEV_IO_STATUS_MISCOMPARE => IoStatus::Miscompare,
            SPDK_BDEV_IO_STATUS_NOMEM => IoStatus::NoMem,
            SPDK_BDEV_IO_STATUS_SCSI_ERROR => IoStatus::ScsiError,
            SPDK_BDEV_IO_STATUS_NVME_ERROR => IoStatus::NvmeError,
            SPDK_BDEV_IO_STATUS_FAILED => IoStatus::Failed,
            SPDK_BDEV_IO_STATUS_PENDING => IoStatus::Pending,
            SPDK_BDEV_IO_STATUS_SUCCESS => IoStatus::Success,
            _ => unreachable!("unexpected spdk_bdev_io_status value"),
        }
    }
}

impl From<Errno> for IoStatus {
    fn from(err: Errno) -> Self {
        match err {
            ENOMEM => IoStatus::NoMem,
            EINPROGRESS => IoStatus::Pending,
            ECANCELED => IoStatus::Aborted,
            _ => IoStatus::Failed,
        }
    }
}

impl From<Result<(), Errno>> for IoStatus {
    fn from(result: Result<(), Errno>) -> Self {
        match result {
            Ok(_) => IoStatus::Success,
            Err(e) => e.into(),
        }
    }
}

impl From<IoStatus> for spdk_bdev_io_status {
    fn from(val: IoStatus) -> spdk_bdev_io_status {
        match val {
            IoStatus::AioError => SPDK_BDEV_IO_STATUS_AIO_ERROR,
            IoStatus::Aborted => SPDK_BDEV_IO_STATUS_ABORTED,
            IoStatus::FirstFusedFailed => SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED,
            IoStatus::Miscompare => SPDK_BDEV_IO_STATUS_MISCOMPARE,
            IoStatus::NoMem => SPDK_BDEV_IO_STATUS_NOMEM,
            IoStatus::ScsiError => SPDK_BDEV_IO_STATUS_SCSI_ERROR,
            IoStatus::NvmeError => SPDK_BDEV_IO_STATUS_NVME_ERROR,
            IoStatus::Failed => SPDK_BDEV_IO_STATUS_FAILED,
            IoStatus::Pending => SPDK_BDEV_IO_STATUS_PENDING,
            IoStatus::Success => SPDK_BDEV_IO_STATUS_SUCCESS,
        }
    }
}

/// A trait for implementing the I/O channel operations for a BDev.
#[async_trait(?Send)]
pub trait BDevIoChannelOps: 'static {
    /// The I/O context type for the BDev.
    type IoContext: Default + 'static;

    /// Submit an I/O request to the BDev.
    async fn submit_request(&self, io: &mut BDevIo<Self::IoContext>) -> Result<(), Errno>;
}

/// A BDev I/O channel implementation.
///
/// The type parameter `T` is the I/O channel context type for the BDev implementation.
pub struct BDevIoChannel<T>
where
    T: BDevIoChannelOps,
{
    channel: NonNull<spdk_io_channel>,
    _ctx: PhantomData<T>,
}

impl<T> BDevIoChannel<T>
where
    T: BDevIoChannelOps,
{
    /// Converts the I/O channel into a raw pointer.
    fn into_raw(self) -> *mut spdk_io_channel {
        self.channel.as_ptr()
    }

    /// Constructs a new I/O channel from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw pointer is non-null and valid.
    unsafe fn from_raw(channel: *mut spdk_io_channel) -> Self {
        Self {
            channel: NonNull::new_unchecked(channel),
            _ctx: PhantomData,
        }
    }

    /// Returns a reference to the I/O channel context.
    pub fn ctx(&self) -> &T {
        unsafe { &*spdk_io_channel_get_ctx(self.channel.as_ptr()).cast() }
    }

    /// Returns a mutable reference to the I/O channel context.
    pub fn ctx_mut(&mut self) -> &mut T {
        unsafe { &mut *spdk_io_channel_get_ctx(self.channel.as_ptr()).cast() }
    }

    /// Returns the thread associated with the I/O channel.
    pub fn thread(&self) -> Thread {
        unsafe { spdk_io_channel_get_thread(self.channel.as_ptr()).into() }
    }
}

impl<T> TryFrom<*mut spdk_io_channel> for BDevIoChannel<T>
where
    T: BDevIoChannelOps,
{
    type Error = Errno;

    fn try_from(channel: *mut spdk_io_channel) -> Result<Self, Self::Error> {
        match NonNull::new(channel) {
            Some(channel) => Ok(Self {
                channel,
                _ctx: PhantomData,
            }),
            None => Err(ENOMEM),
        }
    }
}

/// Represents driver-specific context for an I/O request.
///
/// The type parameter `T` is the I/O context type for the BDev implementation.
#[derive(Default)]
pub(crate) struct BDevIoCtx<T>
where
    T: Default + 'static,
{
    buf_promissory: Option<Arc<Promissory<()>>>,
    inner: T,
}

/// A BDev I/O request.
///
/// The type parameter `T` is the I/O context type for the BDev implementation.
pub struct BDevIo<T>
where
    T: Default + 'static,
{
    io: NonNull<spdk_bdev_io>,
    _ctx: PhantomData<T>,
}

impl<T> BDevIo<T>
where
    T: Default + 'static,
{
    /// Initializes a newly submitted I/O request.
    ///
    /// # Safety
    ///
    /// This function must only be called from the I/O submission callback to
    /// initialize a newly submitted I/O request. It initializes the driver
    /// context to a default value.
    unsafe fn new(io: *mut spdk_bdev_io) -> Self {
        (*io)
            .driver_ctx
            .as_mut_ptr()
            .cast::<BDevIoCtx<T>>()
            .write(Default::default());

        Self {
            io: NonNull::new(io).unwrap(),
            _ctx: PhantomData,
        }
    }
    /// Returns the raw pointer to the I/O request.
    pub fn as_ptr(&self) -> *mut spdk_bdev_io {
        self.io.as_ptr()
    }

    /// Returns the type of the I/O request.
    pub fn io_type(&self) -> IoType {
        (unsafe { self.io.as_ref().type_ as spdk_bdev_io_type }).into()
    }

    /// Returns the thread associated with the I/O request. The I/O request must
    /// be completed on this thread.
    pub fn thread(&self) -> Thread {
        // SAFETY: The thread associated with the I/O request is guaranteed to
        // be non-null and valid.
        unsafe { Thread::from_ptr_unchecked(spdk_bdev_io_get_thread(self.as_ptr())) }
    }

    /// Returns the block device associated with the I/O request.
    pub fn device(&self) -> Device<Any> {
        // SAFETY: The block device associated with the I/O request is
        // guaranteed to be non-null and valid.
        unsafe { Device::<Any>::from_ptr_unchecked(self.io.as_ref().bdev) }
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

    /// Returns a reference to the internal context associated with the I/O
    /// request.
    fn internal_ctx(&self) -> &BDevIoCtx<T> {
        unsafe { &*self.io.as_ref().driver_ctx.as_ptr().cast() }
    }

    /// Returns a mutable reference to the internal context associated with the
    /// I/O request.
    fn internal_ctx_mut(&mut self) -> &mut BDevIoCtx<T> {
        unsafe { &mut *self.io.as_mut().driver_ctx.as_mut_ptr().cast() }
    }

    /// Returns a reference to the implementation-defined context associated
    /// with the I/O request.
    pub fn ctx(&self) -> &T {
        &self.internal_ctx().inner
    }

    /// Returns a mutable reference to the implementation-defined context
    /// associated with the I/O request.
    pub fn ctx_mut(&mut self) -> &mut T {
        &mut self.internal_ctx_mut().inner
    }

    /// Invoked when buffers requested by [`BDevIo<T>::allocate_buffers`] have
    /// been allocated for the I/O request.
    ///
    /// [`BDevIo<T>::allocate_buffers`]: method@BDevIo::allocate_buffers
    unsafe extern "C" fn buffers_allocated(
        _ch: *mut spdk_io_channel,
        io: *mut spdk_bdev_io,
        success: bool,
    ) {
        let mut io: Self = BDevIo::from(io);
        let p = io
            .internal_ctx_mut()
            .buf_promissory
            .take()
            .expect("promissory present");

        Promissory::set_result(p, if_else!(success, Ok(()), Err(EINVAL)));
    }

    /// Allocates buffers aligned to the BDev's requirement for the I/O request.
    ///
    /// Allocation will only occur if no buffers are assigned or the buffers are
    /// not aligned to the BDev's requirement. If the buffers are not aligned,
    /// this call will cause a copy from the current buffers to a bounce buffer
    /// on write or a copy from the bounce buffer to the current buffers on read.
    ///
    /// If no buffers are currently assigned to this I/O request, the `length`
    /// parameter specifies the size of the buffers to allocate in bytes. This
    /// value must be no larger than `SPDK_BDEV_LARGE_BUF_MAX_SIZE`.
    ///
    /// Any buffers allocated by this method will automatically be freed on
    /// completion of this I/O request.
    pub async fn allocate_buffers(&mut self, length: u64) -> Result<(), Errno> {
        Promise::new(move |p| {
            self.internal_ctx_mut().buf_promissory.replace(p.clone());

            unsafe { spdk_bdev_io_get_buf(self.as_ptr(), Some(Self::buffers_allocated), length) };

            Poll::Pending
        })
        .await
    }

    /// Completes the I/O request with the specified status.
    ///
    /// # Panics
    ///
    /// This method panics if not called on the thread associated with the I/O.
    fn complete(mut self, status: IoStatus) {
        assert!(self.thread().is_current());

        unsafe {
            ptr::drop_in_place(self.io.as_mut().driver_ctx.as_mut_ptr() as *mut BDevIoCtx<T>);

            spdk_bdev_io_complete(self.as_ptr(), status.into());
        }
    }
}

impl<T> From<*mut spdk_bdev_io> for BDevIo<T>
where
    T: Default + 'static,
{
    fn from(io: *mut spdk_bdev_io) -> Self {
        Self {
            io: NonNull::new(io).unwrap(),
            _ctx: PhantomData,
        }
    }
}

/// A trait for implementing the BDev operations.
///
/// The type parameter `IoChannel` is the I/O channel type for the BDev.
#[async_trait]
pub trait BDevOps: Send + Sync + 'static {
    type IoChannel: BDevIoChannelOps;

    /// Destroys the BDev.
    async fn destruct(&mut self) -> Result<(), Errno>;

    /// Returns whether the specified I/O type is supported by the BDev.
    fn io_type_supported(&self, io_type: IoType) -> bool;

    /// Gets an I/O channel for the BDev for the calling thread.
    ///
    /// # Notes
    ///
    /// The default implementation returns a per-thread I/O channel for the
    /// BDev. Implementations may override this method to provide different
    /// behavior.
    fn get_io_channel(&self) -> Result<BDevIoChannel<Self::IoChannel>, Errno> {
        unsafe { spdk_get_io_channel(self as *const _ as *mut _).try_into() }
    }

    /// Creates a new I/O channel for the BDev.
    fn new_io_channel(&mut self) -> Result<Self::IoChannel, Errno>;
}

/// A BDev implementation.
///
/// The type parameter `T` is the type that provides the BDev I/O processing implementation.
#[repr(C)]
pub struct BDevImpl<T>
where
    T: BDevOps + ?Sized,
{
    pub bdev: spdk_bdev,
    pub ctx: T,
}

unsafe impl<T> Send for BDevImpl<T> where T: BDevOps + ?Sized {}

unsafe impl<T> Sync for BDevImpl<T> where T: BDevOps + ?Sized {}

impl<T> BDevImpl<T>
where
    T: BDevOps,
{
    /// Creates a new partially initialized BDev instance with the specified
    /// name, owning module and context. Implementors must provider their own
    /// constructor function to complete initialization.
    pub fn new(name: &CStr, module: *const spdk_bdev_module, ctx: T) -> Box<Self> {
        let mut this = Box::new(Self {
            bdev: unsafe { mem::zeroed() },
            ctx,
        });

        this.bdev.ctxt = addr_of_mut!(this.ctx) as *mut c_void;
        this.bdev.name = name.to_owned().into_raw();
        this.bdev.module = module as *mut _;
        this.bdev.fn_table = Self::vtable() as *const _;

        this
    }

    /// Converts a raw `spdk_bdev` pointer to a reference to the BDevImpl.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `spdk_bdev` pointer is valid and points
    /// to a `BDevImpl<T>` instance. This function does not perform any
    /// validation on the pointer.
    pub unsafe fn from_raw(bdev: *mut spdk_bdev) -> &'static BDevImpl<T> {
        &*bdev.byte_sub(offset_of!(BDevImpl<T>, ctx)).cast()
    }

    /// Registers the BDev with the SPDK subsystem. This function must be called
    /// from the SPDK application thread.
    pub fn register(&mut self) -> Result<(), Errno> {
        unsafe {
            spdk_io_device_register(
                self.bdev.ctxt,
                Some(Self::create_io_channel),
                Some(Self::destroy_io_channel),
                size_of::<T::IoChannel>() as u32,
                self.bdev.name,
            );

            if let Err(e) = to_result!(spdk_bdev_register(addr_of_mut!(self.bdev))) {
                spdk_io_device_unregister(self.bdev.ctxt, None);
                return Err(e);
            }
        }

        Ok(())
    }

    /// Unregisters the BDev from the SPDK subsystem. This function must be
    /// called from the SPDK application thread.
    pub async fn unregister(self: Box<Self>) -> Result<(), Errno> {
        let bdev_ptr = self.into_bdev_ptr();

        Promise::new(move |p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

            unsafe {
                spdk_bdev_unregister(bdev_ptr, Some(cb_fn), cb_arg.cast_mut() as *mut _);
            }

            Poll::Pending
        })
        .await
    }

    /// Consumes the boxed instance and returns a [`Device<Owned>`] instance
    /// that owns the BDev.
    pub fn into_device(self: Box<Self>) -> Device<Owned> {
        Device::new(OwnedImpl::new(self)).into_owned().unwrap()
    }

    /// Consumes the boxed BDev instance and returns a raw pointer to the BDev.
    ///
    /// After calling this function, the caller is responsible for managing the
    /// memory previously owned by the boxed BDev instance.
    fn into_bdev_ptr(self: Box<Self>) -> *mut spdk_bdev {
        addr_of_mut!(Box::leak(self).bdev)
    }

    /// Constructs a boxed BDev instance from a raw pointer to the BDev.
    unsafe fn from_ctx_ptr(ctx_ptr: *mut T) -> Box<Self> {
        Box::from_raw(ctx_ptr.byte_sub(offset_of!(BDevImpl<T>, ctx)).cast())
    }

    /// Returns a reference to the BDev context.
    pub fn ctx(&self) -> &T {
        &self.ctx
    }

    /// Returns a mutable reference to the BDev context.
    pub fn ctx_mut(&mut self) -> &mut T {
        &mut self.ctx
    }

    /// Returns the name of the BDev.
    pub fn name(&self) -> &'static CStr {
        unsafe { CStr::from_ptr(self.bdev.name) }
    }

    /// Destroys the BDev instance.
    unsafe extern "C" fn destruct(ctx: *mut c_void) -> i32 {
        thread::spawn_local(async move {
            let mut this = Self::from_ctx_ptr(ctx as *mut T);

            let rc = match this.ctx.destruct().await {
                Ok(_) => 0,
                Err(e) => e.into(),
            };

            unsafe {
                if rc == 0 {
                    spdk_io_device_unregister(this.bdev.ctxt, None);
                }

                spdk_bdev_destruct_done(&this.bdev as *const _ as *mut _, rc);

                if rc != 0 {
                    Box::leak(this);
                }
            }
        });

        1
    }

    /// Creates an I/O channel for the BDev.
    unsafe extern "C" fn create_io_channel(io_device: *mut c_void, ctx_buf: *mut c_void) -> c_int {
        let this = &mut *io_device.cast::<T>();
        let ctx = ctx_buf as *mut T::IoChannel;

        match this.new_io_channel() {
            Ok(channel) => {
                ctx.write(channel);
                0
            }
            Err(e) => e.into(),
        }
    }

    /// Destroys an I/O channel for the BDev.
    unsafe extern "C" fn destroy_io_channel(_io_device: *mut c_void, ctx_buf: *mut c_void) {
        let ctx = ctx_buf as *mut T::IoChannel;

        drop_in_place(ctx);
    }

    /// Returns whether the specified I/O type is supported by the BDev.
    unsafe extern "C" fn io_type_supported(ctx: *mut c_void, io_type: spdk_bdev_io_type) -> bool {
        let this = &*ctx.cast::<T>();

        this.io_type_supported(io_type.into())
    }

    /// Submits an I/O request to the BDev.
    unsafe extern "C" fn submit_request(io_channel: *mut spdk_io_channel, io: *mut spdk_bdev_io) {
        let io_channel = BDevIoChannel::<T::IoChannel>::from_raw(io_channel);
        let mut io = BDevIo::new(io);

        thread::spawn_local(async move {
            let res = io_channel.ctx().submit_request(&mut io).await;

            io.complete(res.into());
        });
    }

    /// Gets an I/O channel for the BDev for the calling thread.
    unsafe extern "C" fn get_io_channel(ctx: *mut c_void) -> *mut spdk_io_channel {
        let this = ctx.cast::<T>();

        (*this)
            .get_io_channel()
            .map_or(ptr::null_mut(), |channel| channel.into_raw())
    }

    fn vtable() -> &'static spdk_bdev_fn_table {
        &spdk_bdev_fn_table {
            io_type_supported: Some(Self::io_type_supported),
            submit_request: Some(Self::submit_request),
            get_io_channel: Some(Self::get_io_channel),
            destruct: Some(Self::destruct),
            dump_info_json: None,
            write_config_json: None,
            get_spin_time: None,
            get_module_ctx: None,
            get_memory_domains: None,
            reset_device_stat: None,
            dump_device_stat_json: None,
            accel_sequence_supported: None,
        }
    }
}

impl<T> Drop for BDevImpl<T>
where
    T: BDevOps + ?Sized,
{
    fn drop(&mut self) {
        unsafe { drop(CString::from_raw(self.bdev.name)) }
    }
}

/// A wrapper that enables [`Device`] to own a custom BDev implementation.
pub(crate) struct OwnedImpl<T: BDevOps>(Box<BDevImpl<T>>);

unsafe impl<T: BDevOps> Send for OwnedImpl<T> {}

impl<T: BDevOps> OwnedImpl<T> {
    /// Creates a new owned BDev instance with the specified BDev implementation.
    pub(crate) fn new(bdev: Box<BDevImpl<T>>) -> Self {
        Self(bdev)
    }
}

#[async_trait]
impl<T: BDevOps> OwnedOps for OwnedImpl<T> {
    fn as_ptr(&self) -> *mut spdk_bdev {
        addr_of!(self.0.bdev) as *mut _
    }

    async fn destroy(self) -> Result<(), Errno> {
        // The BDev implementation's `destruct` method is invoked by the called
        // to unregister the device and will take care of dropping the box. We
        // avoid dropping the box here to prevent double-free.
        let bdev = ManuallyDrop::new(self);

        Promise::new(move |p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

            unsafe {
                spdk_bdev_unregister(bdev.as_ptr(), Some(cb_fn), cb_arg.cast_mut() as *mut _);
            }

            Poll::Pending
        })
        .await
    }
}

impl<T: BDevOps> From<Owned> for OwnedImpl<T> {
    fn from(value: Owned) -> Self {
        Self(unsafe { Box::from_raw(value.as_ptr().cast()) })
    }
}
