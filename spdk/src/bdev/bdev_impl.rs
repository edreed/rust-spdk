use std::{
    error::Error,
    ffi::{CStr, CString},
    future::Future,
    marker::PhantomData,
    mem::{self, ManuallyDrop, MaybeUninit, offset_of, size_of, transmute},
    os::raw::{c_int, c_void},
    pin::Pin,
    ptr::{self, addr_of, addr_of_mut, drop_in_place},
    task::Poll,
};

use spdk_sys::{
    spdk_bdev, spdk_bdev_destruct_done, spdk_bdev_fn_table, spdk_bdev_io, spdk_bdev_io_type,
    spdk_bdev_register, spdk_bdev_unregister, spdk_dif_pi_format_get_size, spdk_get_io_channel,
    spdk_io_channel, spdk_io_device_register, spdk_io_device_unregister, spdk_uuid_copy,
};

use crate::{
    Result, Uuid,
    bdev::{AsRawBDev, BDev, DifCheckFlag, DifPiFormat, DifType, IoError, IoType, Owned, OwnedOps},
    errors::Errno,
    thread::{self, Promise, Promissory},
    to_result,
};

use super::{IoChannelImpl, IoRequest, Module, ModuleOps, io_request::IoRequestCtx};
/// A trait for implementing the BDev operations.
///
/// The type parameter `IoChannel` is the I/O channel type for the BDev.
pub trait BDevOps: Send + Sync {
    /// The per-I/O channel context type for the BDev.
    type ChannelContext;

    /// The per-I/O context type for the BDev.
    type IoContext: Default;

    /// The error type returned by the [`handle_io`] method.
    ///
    /// [`handle_io`]: Self::handle_io
    type IoError: Error + Into<IoError>;

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
    fn get_io_channel(&self) -> Result<IoChannelImpl<Self::ChannelContext>> {
        IoChannelImpl::from_raw(unsafe { spdk_get_io_channel(self as *const _ as *mut _) })
    }

    /// Creates a new I/O channel for the BDev.
    fn new_io_channel(&mut self) -> Result<Self::ChannelContext>;

    /// Returns the size in bytes of the per-I/O context.
    fn get_io_context_size() -> usize
    where
        <Self as BDevOps>::IoContext: 'static,
    {
        size_of::<IoRequestCtx<Self::IoContext>>()
    }

    /// Handles an I/O request to the BDev.
    fn handle_io(
        this: &BDevImpl<Self>,
        channel: &IoChannelImpl<Self::ChannelContext>,
        io: &mut IoRequest<Self::IoContext>,
    ) -> impl Future<Output = std::result::Result<(), Self::IoError>>
    where
        Self: Sized;
}

/// A stub trait implementation for the `BDevOps` trait that enables in-place initialization of a
/// BDev's context
impl<T> BDevOps for MaybeUninit<T>
where
    T: BDevOps,
{
    type ChannelContext = ();
    type IoContext = ();
    type IoError = Errno;

    async fn destruct(&mut self) -> Result<()> {
        unimplemented!("destruct called on uninitialized value");
    }

    fn io_type_supported(&self, _io_type: IoType) -> bool {
        unimplemented!("io_type_supported called on uninitialized value");
    }

    fn new_io_channel(&mut self) -> Result<Self::ChannelContext> {
        unimplemented!("new_io_channel called on uninitialized value");
    }

    async fn handle_io(
        _this: &BDevImpl<Self>,
        _channel: &IoChannelImpl<Self::ChannelContext>,
        _io: &mut IoRequest<Self::IoContext>,
    ) -> Result<()> {
        unimplemented!("submit_request called on uninitialized value");
    }
}

/// A plug-in BDev implementation.
///
/// The type parameter `T` is a user-defined type that provides the BDev I/O request processing
/// implementation.
pub struct BDevImpl<T>
where
    T: BDevOps,
{
    pub bdev: spdk_bdev,
    pub ctx: T,
}

unsafe impl<T> Send for BDevImpl<T> where T: BDevOps + Send {}
unsafe impl<T> Sync for BDevImpl<T> where T: BDevOps + Sync {}

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
    pub unsafe fn from_raw<'a>(bdev: *mut spdk_bdev) -> &'a BDevImpl<T> {
        unsafe { &*bdev.byte_sub(offset_of!(BDevImpl<T>, bdev)).cast() }
    }

    /// Registers the BDev with the SPDK subsystem. This function must be called from the SPDK
    /// application thread.
    ///
    /// # Panics
    ///
    /// This method panics if called from a thread other than the SPDK application thread.
    pub async fn register(&mut self) -> Result<()> {
        assert!(
            thread::is_application(),
            "register must be called from the application thread"
        );

        unsafe {
            spdk_io_device_register(
                self.bdev.ctxt,
                Some(Self::create_io_channel),
                Some(Self::destroy_io_channel),
                size_of::<T::ChannelContext>() as u32,
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

    /// Consumes the boxed instance and returns a [`BDev<Owned>`] instance that owns the BDev.
    pub fn into_device(self: Box<Self>) -> BDev<Owned> {
        BDev::new(OwnedImpl::new(self)).into_owned()
    }

    /// Consumes the boxed BDev instance and returns a raw pointer to the BDev.
    ///
    /// After calling this function, the caller is responsible for managing the memory previously
    /// owned by the boxed BDev instance.
    fn into_bdev_ptr(self: Box<Self>) -> *mut spdk_bdev {
        addr_of_mut!(Box::leak(self).bdev)
    }

    /// Constructs a boxed BDev instance from a raw pointer to the BDev.
    ///
    /// # Safety
    ///
    /// The caller must ensure the pointer is non-null and poitns to a valid [`BDevImpl<T>`] instance.
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
        let destruct_fut = async move {
            // SAFETY: The `ctx` pointer is from the `spdk_bdev`'s `ctxt` field which is
            // initialized to point to this instance's `ctx` field.
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
        };

        // SAFETY: By returning `1` from this function, we inform the SPDK that the destruction is
        // asynchronous. The SPDK guarantees that the current thread lives until the
        // `spdk_bdev_destruct_done` function is called and the future completes.
        unsafe {
            thread::spawn_local_detached(destruct_fut);
        }

        1
    }

    /// Creates an I/O channel for the BDev.
    unsafe extern "C" fn create_io_channel(io_device: *mut c_void, ctx_buf: *mut c_void) -> c_int {
        let this = unsafe { &mut *io_device.cast::<T>() };
        let ctx = ctx_buf as *mut T::ChannelContext;

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
            drop_in_place(ctx_buf as *mut T::ChannelContext);
        }
    }

    /// Returns whether the specified I/O type is supported by the BDev.
    unsafe extern "C" fn io_type_supported(ctx: *mut c_void, io_type: spdk_bdev_io_type) -> bool {
        let this = unsafe { &*ctx.cast::<T>() };

        this.io_type_supported(io_type.into())
    }

    /// Submits an I/O request to the BDev.
    ///
    /// # Panics
    ///
    /// This method panics on debug builds if not called on the thread associated with the I/O.
    unsafe extern "C" fn submit_request(io_channel: *mut spdk_io_channel, io: *mut spdk_bdev_io)
    where
        <T as BDevOps>::IoContext: 'static,
        <T as BDevOps>::ChannelContext: 'static,
    {
        // SAFETY: Both the `io_channel` and `io` pointers are guaranteed to be non-null and valid
        // for the lifetime of the I/O request.
        let io_channel = unsafe { IoChannelImpl::from_raw_unchecked(io_channel) };
        let mut io = unsafe { IoRequest::new_in_place(io) };

        debug_assert!(io.thread().is_current());

        let submit_fut = async move {
            let this = unsafe { Self::from_raw(io.bdev()) };
            let res = T::handle_io(this, &io_channel, &mut io).await;

            // SAFETY: The I/O is completed on the submission thread.
            unsafe { io.complete(res.map_err(Into::into)) };
        };

        // SAFETY: The SPDK guarantees that the current thread lives until the I/O completes.
        unsafe { thread::spawn_local_detached(submit_fut) };
    }

    /// Gets an I/O channel for the BDev for the calling thread.
    unsafe extern "C" fn get_io_channel(ctx: *mut c_void) -> *mut spdk_io_channel {
        let this = unsafe { &mut *ctx.cast::<T>() };

        this.get_io_channel()
            .map_or(ptr::null_mut(), |channel| channel.into_raw())
    }

    fn vtable() -> &'static spdk_bdev_fn_table
    where
        <T as BDevOps>::IoContext: 'static,
        <T as BDevOps>::ChannelContext: 'static,
    {
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
    T: BDevOps,
{
    fn drop(&mut self) {
        unsafe { drop(CString::from_raw(self.bdev.name)) }
    }
}

/// A builder used to create a new instance of a plug-in BDev based on [`BDevImpl`].
///
/// Create a new instance by calling the [`new_bdev_builder()`] method of the [`Module`] managing
/// the plug-in BDev.
///
/// [`new_bdev_builder()`]: super::ModuleOps::new_bdev_builder
pub struct Builder<'a, C, M>
where
    C: BDevOps,
    M: ModuleOps,
{
    module: &'a Module<M>,
    name: &'a CStr,
    uuid: Option<Uuid>,
    block_size: u32,
    num_blocks: u64,
    write_cache_present: bool,
    physical_block_size: u32,
    buffer_alignment: u8,
    optimal_io_boundary: u32,
    metadata_size: Option<u32>,
    is_metadata_interleaved: bool,
    dif_type: DifType,
    dif_pi_format: Option<DifPiFormat>,
    dif_is_head_of_md: bool,
    dif_check_flags: DifCheckFlag,
    numa_id: Option<i32>,

    _phantom: PhantomData<C>,
}

impl<'a, C, M> Builder<'a, C, M>
where
    C: BDevOps,
    M: ModuleOps,
{
    /// Creates a new [`Builder`] instance.
    pub(crate) fn new(
        module: &'a Module<M>,
        name: &'a CStr,
        block_size: u32,
        num_blocks: u64,
    ) -> Self {
        Self {
            module,
            name,
            uuid: None,
            block_size,
            num_blocks,
            write_cache_present: false,
            physical_block_size: 0,
            buffer_alignment: 0,
            optimal_io_boundary: 0,
            metadata_size: None,
            is_metadata_interleaved: false,
            dif_type: DifType::Disabled,
            dif_is_head_of_md: false,
            dif_pi_format: None,
            dif_check_flags: DifCheckFlag::empty(),
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
    unsafe fn init_bdev(&self, bdev: &mut spdk_bdev, ctx: *mut c_void)
    where
        <C as BDevOps>::IoContext: 'static,
        <C as BDevOps>::ChannelContext: 'static,
    {
        bdev.ctxt = ctx;
        bdev.name = self.name.to_owned().into_raw();

        if let Some(uuid) = &self.uuid {
            unsafe { spdk_uuid_copy(&mut bdev.uuid, uuid.as_ptr()) };
        }

        bdev.product_name = M::product_name().as_ptr() as *mut _;
        bdev.module = self.module.as_ptr();
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
        bdev.dif_type = self.dif_type.into();
        bdev.__bindgen_anon_1
            .set_dif_is_head_of_md(self.dif_is_head_of_md as u32);
        bdev.dif_pi_format = self.dif_pi_format.unwrap_or(DifPiFormat::Guard16).into();
        bdev.dif_check_flags = self.dif_check_flags.bits();

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
        let min_md_size = self
            .dif_pi_format
            .map(|f| unsafe { spdk_dif_pi_format_get_size(f.into()) })
            .unwrap_or(0);

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
    /// - `type`: A value from the [`DifType`] enum specifying the DIF type. If
    ///   `DifType::Disabled`, DIF is disabled for this block device and the remaining parameters are
    ///   ignored.
    /// - `pi_format`: An optional value from the [`DifPiFormat`] enum specifying the
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
        r#type: DifType,
        pi_format: Option<DifPiFormat>,
        is_head_of_md: bool,
        check_flags: DifCheckFlag,
    ) -> Self {
        assert!(
            self.dif_type == DifType::Disabled,
            "DIF parameters can only be set once"
        );

        self.dif_type = r#type;

        if self.dif_type != DifType::Disabled {
            let min_md_size = pi_format
                .map(|f| unsafe { spdk_dif_pi_format_get_size(f.into()) })
                .expect("PI format must be specified if DIF is enabled");

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

impl<'a, C, M> Builder<'a, C, M>
where
    C: BDevOps + Default + Unpin,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance with the context initialized to default values.
    pub async fn build(self) -> Result<BDev<Owned>>
    where
        <C as BDevOps>::IoContext: 'static,
        <C as BDevOps>::ChannelContext: 'static,
    {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx: C::default(),
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed and context initialized at this
        // point.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        bdev.register().await?;

        Ok(bdev.into_device())
    }
}

impl<'a, C, M> Builder<'a, C, M>
where
    C: BDevOps + Unpin,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance with the specified context.
    pub async fn build_with_context(self, ctx: C) -> Result<BDev<Owned>>
    where
        <C as BDevOps>::IoContext: 'static,
        <C as BDevOps>::ChannelContext: 'static,
    {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx,
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed and context initialized at this
        // point.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        bdev.register().await?;

        Ok(bdev.into_device())
    }
}

impl<'a, C, M> Builder<'a, C, M>
where
    C: BDevOps,
    M: ModuleOps,
{
    /// Builds and registers a new `BDev` instance, initializing the context in-place using the
    /// specified initialization function.
    pub async fn build_with_context_in_place<I>(self, init_fn: I) -> Result<BDev<Owned>>
    where
        I: AsyncFnOnce(Pin<&mut MaybeUninit<C>>) -> Result<()>,
        <C as BDevOps>::IoContext: 'static,
        <C as BDevOps>::ChannelContext: 'static,
    {
        let mut bdev = Box::new(BDevImpl {
            bdev: unsafe { mem::zeroed() },
            ctx: MaybeUninit::<C>::uninit(),
        });

        // SAFETY: The `BDev`'s `spdk_bdev` struct has been zeroed. It's context will be initialized
        // on the line following this call and before the `BDev` is available globally.
        unsafe { self.init_bdev(&mut bdev.bdev, addr_of_mut!(bdev.ctx) as *mut _) };

        init_fn(unsafe { Pin::new_unchecked(&mut bdev.ctx) }).await?;

        // SAFETY: The `BDev` is fully initialized at this point.
        let mut bdev =
            unsafe { transmute::<Box<BDevImpl<MaybeUninit<C>>>, Box<BDevImpl<C>>>(bdev) };

        bdev.register().await?;

        Ok(bdev.into_device())
    }
}

/// A wrapper that enables [`BDev`] to own a plug-in BDev implementation.
#[repr(transparent)]
struct OwnedImpl<T: BDevOps>(Box<BDevImpl<T>>);

unsafe impl<T: BDevOps> Send for OwnedImpl<T> {}

impl<T: BDevOps> OwnedImpl<T> {
    /// Creates a new owned BDev instance with the specified BDev implementation.
    fn new(bdev: Box<BDevImpl<T>>) -> Self {
        unsafe { transmute(bdev) }
    }
}

impl<T: BDevOps> AsRawBDev for OwnedImpl<T> {
    fn as_raw_bdev(&self) -> *mut spdk_bdev {
        addr_of!(self.0.bdev) as *mut _
    }
}

impl<T: BDevOps> OwnedOps for OwnedImpl<T> {
    async fn destroy(self) -> Result<()> {
        // The BDev implementation's `destruct` method is invoked by the call to unregister the
        // device and will take care of dropping the box. We avoid dropping the box here to prevent
        // double-free.
        let bdev = ManuallyDrop::new(self);

        Promise::new()
            .request(move |p| {
                let (cb_fn, cb_arg) = Promissory::callback_with_status(p);

                unsafe {
                    spdk_bdev_unregister(
                        bdev.as_raw_bdev(),
                        Some(cb_fn),
                        cb_arg.cast_mut() as *mut _,
                    );
                }

                Poll::Pending
            })
            .await
    }
}

impl<T: BDevOps> From<Owned> for OwnedImpl<T> {
    fn from(value: Owned) -> Self {
        Self(unsafe { Box::from_raw(value.as_raw_bdev().cast()) })
    }
}
