use std::{
    ffi::{
        CStr,
        CString,
    },
    io::{
        IoSlice,
        IoSliceMut,
    },
    marker::PhantomData,
    mem::{
        self,

        ManuallyDrop,

        offset_of,

        size_of,
    },
    os::raw::{
        c_int,
        c_void,
    },
    ptr::{
        self,
        NonNull,

        addr_of,
        addr_of_mut,

        drop_in_place,
    },
    slice,
    task::Poll,
};

use async_trait::async_trait;
use spdk_sys::{
    spdk_bdev,
    spdk_bdev_fn_table,
    spdk_bdev_io,
    spdk_bdev_io_status,
    spdk_bdev_io_type,
    spdk_bdev_module,
    spdk_io_channel,

    SPDK_BDEV_IO_STATUS_ABORTED,
    SPDK_BDEV_IO_STATUS_AIO_ERROR,
    SPDK_BDEV_IO_STATUS_FAILED,
    SPDK_BDEV_IO_STATUS_FIRST_FUSED_FAILED,
    SPDK_BDEV_IO_STATUS_MISCOMPARE,
    SPDK_BDEV_IO_STATUS_NOMEM,
    SPDK_BDEV_IO_STATUS_NVME_ERROR,
    SPDK_BDEV_IO_STATUS_PENDING,
    SPDK_BDEV_IO_STATUS_SCSI_ERROR,
    SPDK_BDEV_IO_STATUS_SUCCESS,
    SPDK_BDEV_IO_TYPE_ABORT,
    SPDK_BDEV_IO_TYPE_COMPARE,
    SPDK_BDEV_IO_TYPE_COMPARE_AND_WRITE,
    SPDK_BDEV_IO_TYPE_COPY,
    SPDK_BDEV_IO_TYPE_FLUSH,
    SPDK_BDEV_IO_TYPE_GET_ZONE_INFO,
    SPDK_BDEV_IO_TYPE_INVALID,
    SPDK_BDEV_IO_TYPE_NVME_ADMIN,
    SPDK_BDEV_IO_TYPE_NVME_IO,
    SPDK_BDEV_IO_TYPE_NVME_IO_MD,
    SPDK_BDEV_IO_TYPE_READ,
    SPDK_BDEV_IO_TYPE_RESET,
    SPDK_BDEV_IO_TYPE_SEEK_DATA,
    SPDK_BDEV_IO_TYPE_SEEK_HOLE,
    SPDK_BDEV_IO_TYPE_UNMAP,
    SPDK_BDEV_IO_TYPE_WRITE,
    SPDK_BDEV_IO_TYPE_WRITE_ZEROES,
    SPDK_BDEV_IO_TYPE_ZCOPY,
    SPDK_BDEV_IO_TYPE_ZONE_APPEND,
    SPDK_BDEV_IO_TYPE_ZONE_MANAGEMENT,

    spdk_bdev_destruct_done,
    spdk_bdev_io_complete,
    spdk_bdev_io_get_iovec,
    spdk_bdev_io_get_thread,
    spdk_bdev_register,
    spdk_bdev_unregister,
    spdk_get_io_channel,
    spdk_io_channel_get_ctx,
    spdk_io_channel_get_thread,
    spdk_io_device_register,
    spdk_io_device_unregister,
};

use crate::{
    block::{
        Device,
        Owned,
        OwnedOps,
    },
    errors::Errno,
    runtime::Reactor,
    task::{
        Promise,

        complete_with_status,
    },
    thread::Thread,

    to_result,
};

/// The type of an I/O operation.
/// 
/// # Notes
/// 
/// These are mapped directly to the corresponding [`spdk_bdev_io_type`] values.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum IoType {
    Invalid,
    Read,
    Write,
    Unmap,
    Flush,
    Reset,
    NvmeAdmin,
    NvmeIo,
    NvmeIoMd,
    WriteZeros,
    ZeroCopy,
    GetZoneInfo,
    ZoneManagement,
    ZoneAppend,
    Compare,
    CompareAndWrite,
    Abort,
    SeekHole,
    SeekData,
    Copy,
}

impl From<spdk_bdev_io_type> for IoType {
    fn from(value: spdk_bdev_io_type) -> Self {
        match value {
            SPDK_BDEV_IO_TYPE_INVALID => IoType::Invalid,
            SPDK_BDEV_IO_TYPE_READ => IoType::Read,
            SPDK_BDEV_IO_TYPE_WRITE => IoType::Write,
            SPDK_BDEV_IO_TYPE_UNMAP => IoType::Unmap,
            SPDK_BDEV_IO_TYPE_FLUSH => IoType::Flush,
            SPDK_BDEV_IO_TYPE_RESET => IoType::Reset,
            SPDK_BDEV_IO_TYPE_NVME_ADMIN => IoType::NvmeAdmin,
            SPDK_BDEV_IO_TYPE_NVME_IO => IoType::NvmeIo,
            SPDK_BDEV_IO_TYPE_NVME_IO_MD => IoType::NvmeIoMd,
            SPDK_BDEV_IO_TYPE_WRITE_ZEROES => IoType::WriteZeros,
            SPDK_BDEV_IO_TYPE_ZCOPY => IoType::ZeroCopy,
            SPDK_BDEV_IO_TYPE_GET_ZONE_INFO => IoType::GetZoneInfo,
            SPDK_BDEV_IO_TYPE_ZONE_MANAGEMENT => IoType::ZoneManagement,
            SPDK_BDEV_IO_TYPE_ZONE_APPEND => IoType::ZoneAppend,
            SPDK_BDEV_IO_TYPE_COMPARE => IoType::Compare,
            SPDK_BDEV_IO_TYPE_COMPARE_AND_WRITE => IoType::CompareAndWrite,
            SPDK_BDEV_IO_TYPE_ABORT => IoType::Abort,
            SPDK_BDEV_IO_TYPE_SEEK_HOLE => IoType::SeekHole,
            SPDK_BDEV_IO_TYPE_SEEK_DATA => IoType::SeekData,
            SPDK_BDEV_IO_TYPE_COPY => IoType::Copy,
            _ => unreachable!("unexpected spdk_bdev_io_type value")
        }
    }
}

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
            _ => unreachable!("unexpected spdk_bdev_io_status value")
        }
    }
}

impl Into<spdk_bdev_io_status> for IoStatus {
    fn into(self) -> spdk_bdev_io_status {
        match self {
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
pub trait BDevIoChannelOps: Default + 'static {
    /// The I/O context type for the BDev.
    type IoContext: Default + 'static;

    /// Submit an I/O request to the BDev.
    fn submit_request(&self, io: BDevIo<Self::IoContext>);
}

/// A BDev I/O channel implementation.
/// 
/// The type parameter `T` is the I/O channel context type for the BDev implementation.
pub struct BDevIoChannel<T>
where
    T: BDevIoChannelOps
{
    channel: NonNull<spdk_io_channel>,
    _ctx: PhantomData<T>
}

impl <T> BDevIoChannel<T>
where
    T: BDevIoChannelOps
{
    /// Returns the raw pointer to the I/O channel.
    pub(crate) fn as_ptr(&self) -> *mut spdk_io_channel {
        self.channel.as_ptr()
    }

    /// Returns a reference to the I/O channel context.
    pub fn ctx(&self) -> &T {
        unsafe { &*spdk_io_channel_get_ctx(self.as_ptr()).cast() }
    }

    /// Returns a mutable reference to the I/O channel context.
    pub fn ctx_mut(&mut self) -> &mut T {
        unsafe { &mut *spdk_io_channel_get_ctx(self.as_ptr()).cast() }
    }

    /// Returns the thread associated with the I/O channel.
    pub fn thread(&self) -> Thread {
        unsafe { spdk_io_channel_get_thread(self.as_ptr()).into() }
    }
}

impl <T> From<*mut spdk_io_channel> for BDevIoChannel<T>
where
    T: BDevIoChannelOps
{
    fn from(channel: *mut spdk_io_channel) -> Self {
        Self {
            channel: NonNull::new(channel).unwrap(),
            _ctx: PhantomData,
        }
    }
}

impl <T> Into<*mut spdk_io_channel> for BDevIoChannel<T>
where
    T: BDevIoChannelOps
{
    fn into(self) -> *mut spdk_io_channel {
        self.as_ptr()
    }
}

/// A BDev I/O request.
/// 
/// The type parameter `T` is the I/O context type for the BDev implementation.
pub struct BDevIo<T>
where
    T: 'static
{
    io: NonNull<spdk_bdev_io>,
    _ctx: PhantomData<T>
}

unsafe impl <T: 'static> Send for BDevIo<T> {}

impl <T> BDevIo<T>
where
    T: Default + 'static
{
    /// Initializes a newly submitted I/O request.
    /// 
    /// # Safety
    /// 
    /// This function must only be called from the I/O submission callback to
    /// initialize a newly submitted I/O request. It initializes the driver
    /// context to a default value.
    unsafe fn new(io: *mut spdk_bdev_io) -> Self {
        (*io).driver_ctx.as_mut_ptr().cast::<T>().write(Default::default());

        Self {
            io: NonNull::new(io).unwrap(),
            _ctx: PhantomData
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
        (unsafe { spdk_bdev_io_get_thread(self.as_ptr())}).into()
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
    pub fn buffers_mut(&self) -> &mut [IoSliceMut<'_>] {
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

    /// Returns the context associated with the I/O request.
    pub fn ctx(&self) -> &T {
        unsafe { &*self.io.as_ref().driver_ctx.as_ptr().cast() }
    }

    /// Returns the mutable context associated with the I/O request.
    pub fn ctx_mut(&mut self) -> &mut T {
        unsafe { &mut *self.io.as_mut().driver_ctx.as_mut_ptr().cast() }
    }

    /// Completes the I/O request with the specified status. The I.O request
    /// must be completed on the thread returned by the [`thread()`] method.
    /// 
    /// [`thread()`]: fn@BDevIo::thread
    pub fn complete(self, status: IoStatus) {
        let io_thread = self.thread();

        if io_thread.is_current() {
            unsafe { spdk_bdev_io_complete(self.as_ptr(), status.into()); }
        } else {
            io_thread.send_msg(move || { self.complete(status) }).unwrap();
        }
    }
}

impl<T> From<*mut spdk_bdev_io> for BDevIo<T> {
    fn from(io: *mut spdk_bdev_io) -> Self {
        Self {
            io: NonNull::new(io).unwrap(),
            _ctx: PhantomData
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
    /// BDev. Implementations may override this method to provide a different
    /// behavior.
    fn get_io_channel(&self) -> BDevIoChannel<Self::IoChannel> {
        unsafe {
            spdk_get_io_channel(self as *const _ as *mut _).into()
        }
    }

    /// Prepares a new I/O channel for the BDev.
    /// 
    /// # Notes
    /// 
    /// The default implementation relies on the [`Default`] trait to initialize
    /// the I/O channel. Implementations may override this to peform more
    /// complex initialization.
    fn prepare_io_channel(&mut self, _channel: &mut Self::IoChannel) {}

    /// Releases the resources associated with the I/O channel.
    /// 
    /// # Notes
    /// 
    /// Implementations should generally rely on the [`Drop`] trait to release
    /// resources associated with the I/O channel. Implementations may override
    /// to perform cleanup that cannot be done otherwise.
    fn cleanup_io_channel(&mut self, _channel: &mut Self::IoChannel) {}
}

/// A BDev implementation.
/// 
/// The type parameter `T` is the type that provides the BDev I/O processing implementation.
#[repr(C)]
pub struct BDevImpl<T>
where
    T: BDevOps + ?Sized
{
    pub bdev: spdk_bdev,
    pub ctx: T
}

unsafe impl <T> Send for BDevImpl<T>
where
    T: BDevOps + ?Sized
{
}

unsafe impl <T> Sync for BDevImpl<T>
where
    T: BDevOps + ?Sized
{
}

impl <T> BDevImpl<T>
where
    T: BDevOps
{
    /// Creates a new partially initialized BDev instance with the specified
    /// name, owning module and context. Implementors must provider their own
    /// constructor function to complete initialization.
    pub fn new(name: &CStr, module: *const spdk_bdev_module, ctx: T) -> Box<Self> {
        let mut this = Box::new(Self {
            bdev: unsafe { mem::zeroed() },
            ctx: ctx
        });

        this.bdev.ctxt = addr_of_mut!(this.ctx) as *mut c_void;
        this.bdev.name = name.to_owned().into_raw();
        this.bdev.module = module as *mut _;
        this.bdev.fn_table = Self::vtable() as *const _;

        this
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
                self.bdev.name);

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

        Promise::new(
            move |cx: *mut c_void| {
                unsafe {
                    spdk_bdev_unregister(bdev_ptr, Some(complete_with_status), cx)
                }

                Poll::Pending
            }
        ).await
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
        unsafe { &CStr::from_ptr(self.bdev.name) }
    }

    /// Destroys the BDev instance.
    unsafe extern "C" fn destruct(ctx: *mut c_void) -> i32 {
        Reactor::current().spawn(async move {
            let mut this = Self::from_ctx_ptr(ctx as *mut T);

            let rc = match this.ctx.destruct().await {
                Ok(_) => 0,
                Err(e) => e.into()
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

        ctx.write(Default::default());

        this.prepare_io_channel(&mut *ctx);

        0
    }

    /// Destroys an I/O channel for the BDev.
    unsafe extern "C" fn destroy_io_channel(io_device: *mut c_void, ctx_buf: *mut c_void) {
        let this = &mut *io_device.cast::<T>();
        let ctx = ctx_buf as *mut T::IoChannel;

        this.cleanup_io_channel(&mut *ctx);

        drop_in_place(ctx);
    }

    /// Returns whether the specified I/O type is supported by the BDev.
    unsafe extern "C" fn io_type_supported(ctx: *mut c_void, io_type: spdk_bdev_io_type) -> bool {
        let this = ctx.cast::<Self>();

        (*this).ctx.io_type_supported(io_type.into())
    }

    /// Submits an I/O request to the BDev.
    unsafe extern "C" fn submit_request(io_channel: *mut spdk_io_channel, io: *mut spdk_bdev_io) {
        let io_channel: BDevIoChannel<T::IoChannel> = io_channel.into();

        io_channel.ctx().submit_request(BDevIo::new(io));
    }

    /// Gets an I/O channel for the BDev for the calling thread.
    unsafe extern "C" fn get_io_channel(ctx: *mut c_void) -> *mut spdk_io_channel {
        let this = ctx.cast::<T>();

        (*this).get_io_channel().into()
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

impl <T> Drop for BDevImpl<T>
where
    T: BDevOps + ?Sized
{
    fn drop(&mut self) {
        unsafe {
            drop(CString::from_raw(self.bdev.name))
        }
    }
}

impl <T> From<*mut spdk_bdev> for &'static BDevImpl<T>
where
    T: BDevOps
{
    fn from(bdev: *mut spdk_bdev) -> &'static BDevImpl<T> {
        unsafe { &*bdev.byte_sub(offset_of!(BDevImpl<T>, ctx)).cast() }
    }
}

/// A wrapper that enables [`Device`] to own a custom BDev implementation.
pub (crate) struct OwnedImpl<T: BDevOps>(Box<BDevImpl<T>>);

unsafe impl <T: BDevOps> Send for OwnedImpl<T> {}

impl <T: BDevOps> OwnedImpl<T> {
    /// Creates a new owned BDev instance with the specified BDev implementation.
    pub(crate) fn new(bdev: Box<BDevImpl<T>>) -> Self {
        Self(bdev)
    }
}

#[async_trait]
impl <T: BDevOps> OwnedOps for OwnedImpl<T> {
    fn as_ptr(&self) -> *mut spdk_bdev {
        addr_of!(self.0.bdev) as *mut _
    }

    async fn destroy(self) -> Result<(), Errno> {
        // The BDev implementation's `destruct` method is invoked by the called
        // to unregister the device and will take care of dropping the box. We
        // avoid dropping the box here to prevent double-free.
        let bdev = ManuallyDrop::new(self);

        Promise::new(move |cx| {
            unsafe {
                spdk_bdev_unregister(
                    bdev.as_ptr(),
                    Some(complete_with_status),
                    cx);
            }

            Poll::Pending
        }).await
    }
}

impl <T: BDevOps> From<Owned> for OwnedImpl<T> {
    fn from(value: Owned) -> Self {
        Self(unsafe { Box::from_raw(value.as_ptr().cast()) })
    }
}
