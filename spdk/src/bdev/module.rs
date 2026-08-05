use std::{ffi::CStr, fmt::Debug, future::Future, mem::MaybeUninit, pin::Pin};

use spdk_sys::{
    spdk_bdev, spdk_bdev_module, spdk_bdev_module_examine_done, spdk_bdev_module_fini_done,
    spdk_bdev_module_init_done, spdk_bdev_module_list_add,
};

use crate::{
    block::{Any, Device},
    thread::{self},
};

use super::{BDevImpl, BDevOps};

/// A trait implemented by the [`module`] attribute macro to provide access to the singleton
/// [`Module`] instance.
///
/// [`module`]: macro@crate::module
/// [`Module`]: struct@super::Module
pub trait ModuleInstance<T>
where
    T: Default + ModuleOps + 'static,
{
    /// Returns a reference to the singleton instance of the module.
    fn instance() -> &'static Module<T>;

    /// Returns the product name for BDevs created by this module.
    fn product_name() -> &'static CStr;
}

/// A trait defining BDev module operations.
pub trait ModuleOps: ModuleInstance<Self> + Default + 'static {
    /// The BDev type implemented by the module.
    type BDev: BDevOps;

    /// Initializes the module.
    fn init(&self) -> impl Future<Output = ()> {
        async {}
    }

    /// Finalizes the module.
    fn fini(&self) -> impl Future<Output = ()> {
        async {}
    }

    /// Returns the size in bytes of the per-I/O context.
    fn get_io_context_size(&self) -> usize {
        Self::BDev::get_io_context_size()
    }

    /// Examines the specified block device to determine whether it should be claimed by a Virtual
    /// BDev implemented by this module.
    ///
    /// This method provides the first notification to a Virtual BDev module to examine newly-added
    /// block devices and automatically create their own Virtual BDevs. However, no I/O to the
    /// device can be submitted in this method. The decision whether to claim the BDev must be made
    /// synchronously.
    ///
    /// The default implementation claims no devices.
    fn examine_config(&self, _bdev: Device<Any>) {}

    /// Examines the specified block device to determine whether it should be claimed by a Virtual
    /// BDev implemented by this module.
    ///
    /// This method provides the second notification to a Virtual BDev module to examine newly-added
    /// block devices and automatically create their own Virtual BDevs. I/O may be submitted to the
    /// device in this method and the decision whether to claim can be made asynchronously.
    ///
    /// The default implementation claims no devices.
    fn examine_disk(&self, _bdev: Device<Any>) -> impl std::future::Future<Output = ()> {
        async {}
    }

    /// Creates a new partially initialized BDev instance with the specified name and context.
    /// Implementors must provider their own constructor function to complete initialization.
    fn new_bdev<C>(name: &CStr, ctx: C) -> Box<BDevImpl<C>>
    where
        C: BDevOps + Unpin,
    {
        BDevImpl::new(name, Self::instance(), ctx)
    }

    /// Creates a new partially initialized BDev instance with the specified name and initializing
    /// the context in-place. Implementors must provider their own constructor function to complete
    /// initialization.
    fn new_bdev_in_place<C, I>(name: &CStr, init_fn: I) -> Box<BDevImpl<C>>
    where
        C: BDevOps,
        I: FnOnce(Pin<&mut MaybeUninit<C>>),
    {
        BDevImpl::new_in_place(name, Self::instance(), init_fn)
    }
}

/// A BDev module implementation created by the [`module`] attribute macro.
///
/// The type parameter `T` is the module context type.
///
/// [`module`]: macro@crate::module
#[derive(Debug)]
pub struct Module<T>
where
    T: ModuleInstance<T> + Default + ModuleOps + 'static,
{
    module: spdk_bdev_module,
    ctx: T,
}

unsafe impl<T> Send for Module<T> where T: ModuleInstance<T> + Default + ModuleOps + Send + 'static {}

unsafe impl<T> Sync for Module<T> where T: ModuleInstance<T> + Default + ModuleOps + Sync + 'static {}

impl<T> Module<T>
where
    T: ModuleInstance<T> + Default + ModuleOps + 'static,
{
    /// Creates a new BDev module instance with the specified name.
    ///
    /// This method is used by the [`module`] attribute macro to create the singleton module
    /// instance. It must not be called directly.
    ///
    /// [`module`]: macro@crate::module
    pub fn new(name: &'static CStr) -> Self {
        let mut module: spdk_bdev_module = unsafe { std::mem::zeroed() };

        module.name = name.as_ptr();
        module.module_init = Some(Self::init);
        module.module_fini = Some(Self::fini);
        module.get_ctx_size = Some(Self::get_io_context_size);
        module.examine_config = Some(Self::examine_config);
        module.examine_disk = Some(Self::examine_disk);
        module.async_init = true;
        module.async_fini = true;
        module.init_complete = None;
        module.fini_start = None;
        module.config_json = None;
        module.async_fini_start = false;

        Self {
            module,
            ctx: Default::default(),
        }
    }

    /// Registers the module with the SPDK BDev module list.
    ///
    /// This method is used by the [`module`] attribute macro to register the singleton module
    /// instance. It must not be called directly.
    ///
    /// [`module`]: macro@crate::module
    pub fn register(&self) {
        unsafe { spdk_bdev_module_list_add(&self.module as *const _ as *mut _) };
    }

    /// Returns a pointer to the underlying `spdk_bdev_module` structure.
    pub(crate) fn as_ptr(&self) -> *const ::spdk_sys::spdk_bdev_module {
        &self.module as *const _
    }

    /// Returns a reference to the module context.
    pub fn ctx(&self) -> &T {
        &self.ctx
    }

    /// Initializes the module.
    unsafe extern "C" fn init() -> i32 {
        thread::spawn_local(async {
            T::instance().ctx.init().await;

            unsafe {
                spdk_bdev_module_init_done(T::instance().as_ptr() as *mut _);
            }
        });

        0
    }

    /// Finalizes the module.
    unsafe extern "C" fn fini() {
        thread::spawn_local(async {
            T::instance().ctx.fini().await;

            unsafe {
                spdk_bdev_module_fini_done();
            }
        });
    }

    /// Returns the size in bytes of the per-I/O context.
    unsafe extern "C" fn get_io_context_size() -> i32 {
        T::instance().ctx.get_io_context_size() as i32
    }

    /// Examines the specified block device to determine whether it should be claimed by a Virtual
    /// BDev implemented by this module.
    ///
    /// This function provides the first notification to a Virtual BDev module to examine
    /// newly-added block devices and automatically create their own Virtual BDevs. However, no I/O
    /// to the device can be submitted in this function. The decision whether to claim the BDev must
    /// be made synchronously.
    ///
    /// The default implementation claims no devices.
    unsafe extern "C" fn examine_config(bdev: *mut spdk_bdev) {
        T::instance().ctx.examine_config(bdev.into());

        unsafe { spdk_bdev_module_examine_done(T::instance().as_ptr() as *mut _) }
    }

    /// Examines the specified block device to determine whether it should be claimed by a Virtual
    /// BDev implemented by this module.
    ///
    /// This function provides the second notification to a Virtual BDev module to examine
    /// newly-added block devices and automatically create their own Virtual BDevs. I/O may be
    /// submitted to the device in this function and the decision whether to claim can be made
    /// asynchronously.
    ///
    /// The default implementation claims no devices.
    unsafe extern "C" fn examine_disk(bdev: *mut spdk_bdev) {
        let bdev = bdev.into();

        thread::spawn_local(async move {
            T::instance().ctx.examine_disk(bdev).await;

            unsafe { spdk_bdev_module_examine_done(T::instance().as_ptr() as *mut _) }
        });
    }
}
