use std::{
    ffi::{CStr, CString},
    future::Future,
    io::{IoSlice, IoSliceMut},
    marker::PhantomData,
    mem::{self, ManuallyDrop, MaybeUninit, offset_of, size_of, transmute},
    os::raw::{c_int, c_void},
    pin::Pin,
    ptr::{self, NonNull, addr_of, addr_of_mut, drop_in_place},
    rc::{Rc, Weak},
    slice,
    task::Poll,
};

use spdk_sys::{
    SPDK_BDEV_IO_STATUS_ABORTED, SPDK_BDEV_IO_STATUS_AIO_ERROR, SPDK_BDEV_IO_STATUS_FAILED,
    SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED, SPDK_BDEV_IO_STATUS_MISCOMPARE,
    SPDK_BDEV_IO_STATUS_NOMEM, SPDK_BDEV_IO_STATUS_NVME_ERROR, SPDK_BDEV_IO_STATUS_PENDING,
    SPDK_BDEV_IO_STATUS_SCSI_ERROR, SPDK_BDEV_IO_STATUS_SUCCESS, spdk_bdev,
    spdk_bdev_destruct_done, spdk_bdev_fn_table, spdk_bdev_io, spdk_bdev_io_complete,
    spdk_bdev_io_get_buf, spdk_bdev_io_get_iovec, spdk_bdev_io_get_thread, spdk_bdev_io_status,
    spdk_bdev_io_type, spdk_bdev_register, spdk_bdev_unregister,
    spdk_dif_pi_format::{
        self, SPDK_DIF_PI_FORMAT_16, SPDK_DIF_PI_FORMAT_32, SPDK_DIF_PI_FORMAT_64,
    },
    spdk_dif_type::{self, SPDK_DIF_DISABLE},
    spdk_get_io_channel, spdk_io_channel, spdk_io_channel_get_ctx, spdk_io_channel_get_thread,
    spdk_io_device_register, spdk_io_device_unregister,
};
use ternary_rs::if_else;

use crate::{
    Result,
    block::{Any, Device, IoType, Owned, OwnedOps},
    errors::{ECANCELED, EINPROGRESS, EINVAL, ENOMEM, ENOTSUP, Errno},
    task::{Promise, Promissory},
    thread::{self, Thread},
    to_result,
};

use super::{Module, ModuleOps};

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

impl From<Result<()>> for IoStatus {
    fn from(result: Result<()>) -> Self {
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
pub trait BDevIoChannelOps: 'static {
    /// A per-I/O context type accessed through the [`BDevIo::ctx()`] and [`BDevIo::ctx_mut()`]
    /// methods.
    ///
    /// [`BDevIo::ctx()`]: method@super::BDevIo::ctx
    /// [`BDevIo::ctx_mut()`]: method@super::BDevIo::ctx_mut
    type IoContext: Default + 'static;

    /// Submit an I/O request to the BDev.
    fn submit_request(
        &mut self,
        io: &mut BDevIo<Self::IoContext>,
    ) -> impl Future<Output = Result<()>>;
}

/// A stub trait implementation for the `BDevIoChannelOps` trait that does not support any I/O
/// operations.
impl BDevIoChannelOps for () {
    type IoContext = ();

    async fn submit_request(&mut self, _io: &mut BDevIo<Self::IoContext>) -> Result<()> {
        Err(ENOTSUP)
    }
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
            channel: unsafe { NonNull::new_unchecked(channel) },
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

    fn try_from(channel: *mut spdk_io_channel) -> Result<Self> {
        match NonNull::new(channel) {
            Some(channel) => Ok(Self {
                channel,
                _ctx: PhantomData,
            }),
            None => Err(ENOMEM),
        }
    }
}

/// A type alias for the promissory that receives the buffer allocation from a call to the
/// [`BDevIo<T>::allocate_buffers`] method.
type AllocateBuffersPromissory<'a, T> = Promissory<(), PhantomData<&'a mut BDevIo<T>>>;

/// Represents driver-specific context for an I/O request.
///
/// The type parameter `T` is the I/O context type for the BDev implementation.
#[derive(Default)]
struct BDevIoCtx<'a, T>
where
    T: Default + 'static,
{
    buf_promissory: Option<Weak<AllocateBuffersPromissory<'a, T>>>,
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
    /// This function must only be called from the I/O submission callback to initialize a newly
    /// submitted I/O request. It initializes the driver's I/O context to a default value.
    unsafe fn new(io: *mut spdk_bdev_io) -> Self {
        unsafe {
            (*io)
                .driver_ctx
                .as_mut_ptr()
                .cast::<BDevIoCtx<T>>()
                .write(Default::default())
        };

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

    /// Returns the thread associated with the I/O request. The I/O request must be completed on
    /// this thread.
    pub fn thread(&self) -> Thread {
        // SAFETY: The thread associated with the I/O request is guaranteed to be non-null and
        // valid.
        unsafe { Thread::from_ptr_unchecked(spdk_bdev_io_get_thread(self.as_ptr())) }
    }

    /// Returns the block device associated with the I/O request.
    pub fn device(&self) -> Device<Any> {
        // SAFETY: The block device associated with the I/O request is guaranteed to be non-null and
        // valid.
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

    /// Returns a reference to the internal context associated with the I/O request.
    fn internal_ctx(&self) -> &BDevIoCtx<'_, T> {
        unsafe { &*self.io.as_ref().driver_ctx.as_ptr().cast() }
    }

    /// Returns a mutable reference to the internal context associated with the I/O request.
    fn internal_ctx_mut(&mut self) -> &mut BDevIoCtx<'_, T> {
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

    /// Invoked when buffers requested by [`BDevIo<T>::allocate_buffers`] have been allocated for
    /// the I/O request.
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
            .expect("promissory present")
            .upgrade()
            .expect("promissory has strong references");

        Promissory::set_result(p, if_else!(success, Ok(()), Err(EINVAL)));
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
    pub async fn allocate_buffers<'a>(&'a mut self, length: u64) -> Result<()> {
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
pub trait BDevOps: Send + Sync + 'static {
    type IoChannel: BDevIoChannelOps;

    /// Destroys the BDev.
    fn destruct(&mut self) -> impl Future<Output = Result<()>>;

    /// Returns whether the specified I/O type is supported by the BDev.
    fn io_type_supported(&self, io_type: IoType) -> bool;

    /// Gets an I/O channel for the BDev for the calling thread.
    ///
    /// # Notes
    ///
    /// The default implementation returns a per-thread I/O channel for the BDev. Implementations
    /// may override this method to provide different behavior.
    fn get_io_channel(&self) -> Result<BDevIoChannel<Self::IoChannel>> {
        unsafe { spdk_get_io_channel(self as *const _ as *mut _).try_into() }
    }

    /// Creates a new I/O channel for the BDev.
    fn new_io_channel(&mut self) -> Result<Self::IoChannel>;

    /// Returns the size in bytes of the per-I/O context.
    fn get_io_context_size() -> usize {
        size_of::<BDevIoCtx<<<Self as BDevOps>::IoChannel as BDevIoChannelOps>::IoContext>>()
    }
}

/// A stub trait implementation for the `BDevOps` trait that enables in-place initialization of a
/// BDev's context
impl<T> BDevOps for MaybeUninit<T>
where
    T: BDevOps,
{
    type IoChannel = ();

    async fn destruct(&mut self) -> Result<()> {
        unimplemented!("destruct called on uninitialized value");
    }

    fn io_type_supported(&self, _io_type: IoType) -> bool {
        unimplemented!("io_type_supported called on uninitialized value");
    }

    fn new_io_channel(&mut self) -> Result<Self::IoChannel> {
        unimplemented!("new_io_channel called on uninitialized value");
    }
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

unsafe impl<T> Send for BDevImpl<T> where T: BDevOps + Send + ?Sized {}

unsafe impl<T> Sync for BDevImpl<T> where T: BDevOps + Sync + ?Sized {}
impl<T> BDevImpl<T>
where
    T: BDevOps,
{
    /// Converts a raw `spdk_bdev` pointer to a reference to the BDevImpl.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `spdk_bdev` pointer is valid and points to a `BDevImpl<T>`
    /// instance. This function does not perform any validation on the pointer.
    pub unsafe fn from_raw(bdev: *mut spdk_bdev) -> &'static BDevImpl<T> {
        unsafe { &*bdev.byte_sub(offset_of!(BDevImpl<T>, ctx)).cast() }
    }

    /// Registers the BDev with the SPDK subsystem. This function must be called from the SPDK
    /// application thread.
    pub fn register(&mut self) -> Result<()> {
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

    /// Unregisters the BDev from the SPDK subsystem. This function must be called from the SPDK
    /// application thread.
    pub async fn unregister(self: Box<Self>) -> Result<()> {
        let bdev_ptr = self.into_bdev_ptr();

        Promise::new()
            .request(move |p| {
                let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

                unsafe {
                    spdk_bdev_unregister(bdev_ptr, Some(cb_fn), cb_arg.cast_mut() as *mut _);
                }

                Poll::Pending
            })
            .await
    }

    /// Consumes the boxed instance and returns a [`Device<Owned>`] instance that owns the BDev.
    pub fn into_device(self: Box<Self>) -> Device<Owned> {
        Device::new(OwnedImpl::new(self)).into_owned().unwrap()
    }

    /// Consumes the boxed BDev instance and returns a raw pointer to the BDev.
    ///
    /// After calling this function, the caller is responsible for managing the memory previously
    /// owned by the boxed BDev instance.
    fn into_bdev_ptr(self: Box<Self>) -> *mut spdk_bdev {
        addr_of_mut!(Box::leak(self).bdev)
    }

    /// Constructs a boxed BDev instance from a raw pointer to the BDev.
    unsafe fn from_ctx_ptr(ctx_ptr: *mut T) -> Box<Self> {
        unsafe { Box::from_raw(ctx_ptr.byte_sub(offset_of!(BDevImpl<T>, ctx)).cast()) }
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
            let mut this = unsafe { Self::from_ctx_ptr(ctx as *mut T) };

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
        let this = unsafe { &mut *io_device.cast::<T>() };
        let ctx = ctx_buf as *mut T::IoChannel;

        match this.new_io_channel() {
            Ok(channel) => {
                unsafe {
                    ctx.write(channel);
                }
                0
            }
            Err(e) => e.into(),
        }
    }

    /// Destroys an I/O channel for the BDev.
    unsafe extern "C" fn destroy_io_channel(_io_device: *mut c_void, ctx_buf: *mut c_void) {
        unsafe {
            drop_in_place(ctx_buf as *mut T::IoChannel);
        }
    }

    /// Returns whether the specified I/O type is supported by the BDev.
    unsafe extern "C" fn io_type_supported(ctx: *mut c_void, io_type: spdk_bdev_io_type) -> bool {
        let this = unsafe { &*ctx.cast::<T>() };

        this.io_type_supported(io_type.into())
    }

    /// Submits an I/O request to the BDev.
    unsafe extern "C" fn submit_request(io_channel: *mut spdk_io_channel, io: *mut spdk_bdev_io) {
        let mut io_channel = unsafe { BDevIoChannel::<T::IoChannel>::from_raw(io_channel) };
        let mut io = unsafe { BDevIo::new(io) };

        thread::spawn_local(async move {
            let res = io_channel.ctx_mut().submit_request(&mut io).await;

            io.complete(res.into());
        });
    }

    /// Gets an I/O channel for the BDev for the calling thread.
    unsafe extern "C" fn get_io_channel(ctx: *mut c_void) -> *mut spdk_io_channel {
        let this = unsafe { &mut *ctx.cast::<T>() };

        this.get_io_channel()
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
            get_memory_domain_types: None,
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

/// A builder used to create a new instance of a `BDev`.
///
/// Create a new instance by calling the [`new_bdev_builder()`] method of the [`Module`] managing
/// the `BDev` type.
///
/// [`new_bdev_builder()`]: super::ModuleOps::new_bdev_builder
pub struct BDevBuilder<'a, C, M>
where
    C: BDevOps,
    M: ModuleOps,
{
    module: &'a Module<M>,
    name: &'a CStr,
    block_size: u32,
    num_blocks: u64,
    write_cache_present: bool,
    physical_block_size: u32,
    buffer_alignment: u8,
    optimal_io_boundary: u32,
    metadata_size: Option<u32>,
    is_metadata_interleaved: bool,
    dif_type: spdk_dif_type,
    dif_pi_format: Option<spdk_dif_pi_format>,
    dif_is_head_of_md: bool,
    dif_check_flags: u32,
    numa_id: Option<i32>,

    _phantom: PhantomData<C>,
}

impl<'a, C, M> BDevBuilder<'a, C, M>
where
    C: BDevOps,
    M: ModuleOps,
{
    /// Creates a new [`BDevBuilder`] instance.
    pub(crate) fn new(
        module: &'a Module<M>,
        name: &'a CStr,
        block_size: u32,
        num_blocks: u64,
    ) -> Self {
        Self {
            module,
            name,
            block_size,
            num_blocks,
            write_cache_present: false,
            physical_block_size: 0,
            buffer_alignment: 0,
            optimal_io_boundary: 0,
            metadata_size: None,
            is_metadata_interleaved: false,
            dif_type: SPDK_DIF_DISABLE,
            dif_is_head_of_md: false,
            dif_pi_format: None,
            dif_check_flags: 0,
            numa_id: None,
            _phantom: PhantomData,
        }
    }

    /// Initializes the `spdk_bdev` structure of the `BDev` being built.
    ///
    /// # Safety
    ///
    /// This function assumes the memory referenced by the `bdev` argument has been initialized to
    /// zeroes. Failure to do so results in **undefined behavior**.
    ///
    /// The `ctx` argument is a raw pointer to the new `BDev`'s context. It may be uninitialized at
    /// this point, so dereferecing it results in **undefined behavior**. This method simply stores
    /// the pointer value in the `spdk_bdev`'s `ctxt` field. The caller must ensure that the context
    /// is properly initialized before this field can be deferenced.
    unsafe fn init_bdev(&self, bdev: &mut spdk_bdev, ctx: *mut c_void) {
        bdev.ctxt = ctx;
        bdev.name = self.name.to_owned().into_raw();
        bdev.product_name = M::product_name().as_ptr() as *mut _;
        bdev.module = self.module.as_ptr() as *mut _;
        bdev.fn_table = BDevImpl::<C>::vtable() as *const _;
        bdev.write_cache = self.write_cache_present as i32;
        bdev.blocklen = self.block_size;
        bdev.blockcnt = self.num_blocks;
        bdev.phys_blocklen = self.physical_block_size;
        bdev.required_alignment = self.buffer_alignment;
        bdev.optimal_io_boundary = self.optimal_io_boundary;
        bdev.md_len = self.metadata_size.unwrap_or(0);
        bdev.__bindgen_anon_1
            .set_md_interleave(self.is_metadata_interleaved as u32);
        bdev.dif_type = self.dif_type;
        bdev.__bindgen_anon_1
            .set_dif_is_head_of_md(self.dif_is_head_of_md as u32);
        bdev.dif_pi_format = self.dif_pi_format.unwrap_or(SPDK_DIF_PI_FORMAT_16);
        bdev.dif_check_flags = self.dif_check_flags;

        if let Some(numa_id) = self.numa_id {
            bdev.numa.set_id(numa_id);
            bdev.numa.set_id_valid(true as u32);
        }
    }

    /// Set whether the new `BDev` has a write cache.
    pub fn with_write_cache_present(mut self, has_write_cache: bool) -> Self {
        self.write_cache_present = has_write_cache;
        self
    }

    /// Set the new `BDev`'s physical block size.
    ///
    /// If the physical block size is not explcitly set, it is assumed to be the same size of a
    /// logical block.
    pub fn with_physical_block_size(mut self, size: u32) -> Self {
        self.physical_block_size = size;
        self
    }

    /// Set the new `BDev`'s minimum buffer alignment.
    ///
    /// The alignment must be a non-zero power of 2. If not explicitly specified, byte alignment is
    /// assumed.
    ///
    /// # Panics
    ///
    /// The value must be a non-zero power of 2 otherwise this function panics.
    pub fn with_buffer_alignment(mut self, alignment: usize) -> Self {
        assert!(
            alignment.is_power_of_two(),
            "buffer alignment must be a power of two"
        );
        self.buffer_alignment = alignment.checked_ilog2().expect("not zero") as u8;
        self
    }

    /// Set the new `BDev`'s optimal I/O boundary in number of blocks.
    ///
    /// If not explicitly specified, no boundary is assumed.
    pub fn with_optimal_io_boundary(mut self, boundary: u32) -> Self {
        self.optimal_io_boundary = boundary;
        self
    }

    /// Set the new `BDev`'s metadata parameters.
    ///
    /// If not explicitly specified, it is assumed the new `BDev` does not support metadata or
    /// supports only the [Data Integrity Field (DIF)] as specified by the [`Self::with_dif()`]
    /// method.
    ///
    /// # Parameters
    ///
    /// - `size`: The size of the metadata in bytes. A value of zero indicates no metadata is supported.
    /// - `interleaved`: Specifies whether the metadata is interleaved with or separate from the
    ///   block data.
    ///
    /// # Panics
    ///
    /// This function panics if DIF was enabled and the specified metadata size is smaller than the
    /// required number of DIF bytes.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn with_metadata(mut self, size: u32, interleaved: bool) -> Self {
        let min_md_size = match self.dif_pi_format {
            Some(SPDK_DIF_PI_FORMAT_16) => 8,
            Some(SPDK_DIF_PI_FORMAT_32) | Some(SPDK_DIF_PI_FORMAT_64) => 16,
            None => 0,
        };

        assert!(
            size >= min_md_size,
            "the metadata size of {size} bytes is less than the {min_md_size} bytes required by the DIF configuration"
        );

        self.metadata_size = Some(size);
        self.is_metadata_interleaved = interleaved;
        self
    }

    /// Set the new `BDev`'s [Data Integrity Field (DIF)] parameters.
    ///
    /// If the metadata size has not been set, this method sets the metadata size to the number of
    /// bytes required by the DIF configuration.
    ///
    /// # Parameters
    ///
    /// - `type`: A value from the [`spdk_dif_type`] enum specifying the DIF type. If
    ///   `SPDK_DIF_DISABLE`, DIF is disabled for this block device and the remaining parameters are
    ///   ignored.
    /// - `pi_format`: An optional value from the [`spdk_dif_pi_format`] enum specifying the
    ///   protection information format. This parameter is required to be specified as
    ///   `Some(pi_format)` if DIF is not disabled.
    /// - `is_head_of_md`: Specifies whether the DIF is set in the new `BDev`'s first or last 8|16
    ///   bytes of metadata.
    /// - `check_flags`: The bitmap of enabled DIF checks.
    ///
    /// # Panics
    ///
    /// This method panics if the metadata size has already been specified and is less than the
    /// number of bytes required by the DIF configuration.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    /// [`spdk_dif_type`]: spdk_sys::spdk_dif_type
    /// [`spdk_dif_pi_format`]: spdk_sys::spdk_dif_pi_format
    pub fn with_dif(
        mut self,
        r#type: spdk_dif_type,
        pi_format: Option<spdk_dif_pi_format>,
        is_head_of_md: bool,
        check_flags: u32,
    ) -> Self {
        assert!(
            self.dif_type == SPDK_DIF_DISABLE,
            "DIF parameters can only be set once"
        );

        self.dif_type = r#type;

        if self.dif_type != SPDK_DIF_DISABLE {
            let min_md_size = match pi_format {
                Some(SPDK_DIF_PI_FORMAT_16) => 8,
                Some(SPDK_DIF_PI_FORMAT_32) | Some(SPDK_DIF_PI_FORMAT_64) => 16,
                None => panic!("PI format must be specified if DIF is enabled"),
            };

            match self.metadata_size {
                None => self.metadata_size = Some(min_md_size),
                Some(size) => {
                    assert!(
                        size >= min_md_size,
                        "the metadata size of {size} bytes is less than the {min_md_size} bytes required by the DIF configuration"
                    );
                }
            }

            self.dif_pi_format = pi_format;
            self.dif_is_head_of_md = is_head_of_md;
            self.dif_check_flags = check_flags
        }
        self
    }

    /// Set the new `BDev`'s NUMA node ID.
    ///
    /// The specified value may be `None` if the NUMA node ID is not known.
    pub fn with_numa_id(mut self, numa_id: Option<i32>) -> Self {
        self.numa_id = numa_id;
        self
    }
}

impl<'a, C, M> BDevBuilder<'a, C, M>
where
    C: BDevOps + Default + Unpin + 'static,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance with the context initialized to default values.
    pub fn build(self) -> Result<Device<Owned>> {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx: C::default(),
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed and context initialized at this
        // point.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        bdev.register()?;

        Ok(bdev.into_device())
    }
}

impl<'a, C, M> BDevBuilder<'a, C, M>
where
    C: BDevOps + Unpin + 'static,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance with the specified context.
    pub fn build_with_context(self, ctx: C) -> Result<Device<Owned>> {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx,
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed and context initialized at this
        // point.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        bdev.register()?;

        Ok(bdev.into_device())
    }
}

impl<'a, C, M> BDevBuilder<'a, C, M>
where
    C: BDevOps + 'static,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance, initializing the context in-place using the
    /// specified initialization function.
    pub fn build_with_context_in_place<I>(self, init_fn: I) -> Result<Device<Owned>>
    where
        I: FnOnce(Pin<&mut MaybeUninit<C>>),
    {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx: MaybeUninit::<C>::uninit(),
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed. It's context will be initialized
        // on the line following this call and before the `BDev` is available globally.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        init_fn(unsafe { Pin::new_unchecked(&mut bdev.ctx) });

        // SAFETY: The `BDev` is fully initialized at this point.
        let mut bdev =
            unsafe { transmute::<Box<BDevImpl<MaybeUninit<C>>>, Box<BDevImpl<C>>>(bdev) };

        bdev.register()?;

        Ok(bdev.into_device())
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

impl<T: BDevOps> OwnedOps for OwnedImpl<T> {
    fn as_ptr(&self) -> *mut spdk_bdev {
        addr_of!(self.0.bdev) as *mut _
    }

    async fn destroy(self) -> Result<()> {
        // The BDev implementation's `destruct` method is invoked by the call to unregister the
        // device and will take care of dropping the box. We avoid dropping the box here to prevent
        // double-free.
        let bdev = ManuallyDrop::new(self);

        Promise::new()
            .request(move |p| {
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
