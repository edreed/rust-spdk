//! Support for the Storage Performance Development Kit plug-in block devices (BDevs) and modules.
//!
//! # Supported Block Device (BDev) Modules
//!
//! The `rust-spdk` crate currently provides support for the following SPDK-provided block device
//! modules. To use one of these BDevs, add the feature from the `Feature` column to the `rust-spdk`
//! dependency in your `Cargo.toml` file.
//!
//! | Module           | Feature       | Builder           |
//! |------------------|---------------|-------------------|
//! | [Linux AIO BDev] | `bdev-aio`    | [`Aio::new`]      |
//! | [Malloc BDev]    | `bdev-malloc` | [`MallocBuilder`] |
//! | [Uring BDev]     | `bdev-uring`  | [`UringBuilder`]  |
//!
//! See [Block Device User Guide] for more information about the SPDK-provided block device modules
//! in the table above.
//!
//! # Custom Block Device (BDev) Modules Using `rust-spdk`
//!
//! The `rust-spdk` crate supports writing custom block device (BDev) modules in asynchronous Rust.
//! Add the `bdev-module` feature to the `rust-spdk` dependency in your `Cargo.toml` file to enable
//! this support.
//!
//! The first step is to define a `struct` representing the context of your custom BDev. Provide the
//! custom BDev's implementation using the [`BDevOps`] trait. If necessary, you can also provide
//! channel- and/or I/O-specific contexts by defining a `struct` for each and tying them to your
//! custom BDev through the [`BDevOps::ChannelContext`] and [`BDevOps::IoContext`] associated types,
//! respectively. These associated types can be assigned the unit (`()`) type if no additional
//! context is needed by your custom BDev.
//!
//! Next, define a `struct` representing the global module context for your custom BDev. Provide the
//! custom BDev's implementation using the [`ModuleOps`] trait, assigning your custom BDev `struct`
//! to the [`ModuleOps::BDev`] associated type. All of the `ModuleOps` traits methods are provided
//! for you, so you are not required to provide any implementations for it. However, global module
//! initialization and finalization can be performed by providing your own implementations of the
//! [`ModuleOps::init`] and [`ModuleOps::fini`] methods, respectively. Virtual BDevs may also
//! provide implementations of the [`ModuleOps::examine_config`] and [`ModuleOps::examine_disk`]
//! methods to examine and claim ownership of, if necessary, other BDev instances as they are
//! registered with the SPDK BDev subsystem.
//!
//! Finally, decorate your module `struct` with the [`spdk::module`] attribute and start using it
//! from your `rust-spdk` application. The [`ModuleOps::new_bdev_builder`] returns a builder object
//! that you use to create a new instance of your custom BDev.
//!
//! ## Example
//!
//! The following example implements a simple BDev that ignores writes and returns zeroed blocks on
//! read.
//!
//! ```no_run
#![doc = include_str!("../../examples/module_null.rs")]
//!```
//!
//! The [Echo BDev Module] and [PassthruRs Virtual BDev Module] examples demonstrate more complex BDev implementations.
//!
//! See [Writing a Custom Block Device Module] for general information about writing a custom
//! plug-in block device modules for the SPDK.
//!
//! [Linux AIO BDev]: crate::bdev::Aio
//! [Malloc BDev]: crate::bdev::Malloc
//! [Uring BDev]: crate::bdev::Uring
//! [Block Device User Guide]: https://spdk.io/doc/bdev.html
//! [Writing a Custom Block Device Module]: https://spdk.io/doc/bdev_module.html
//! [`spdk::module`]: crate::module
//! [Echo BDev Module]: https://github.com/edreed/rust-spdk/blob/main/spdk/examples/module_echo.rs
//! [PassthruRs Virtual BDev Module]: https://github.com/edreed/rust-spdk/blob/main/spdk/examples/module_passthru.rs
#![cfg(feature = "bdev")]
#![cfg_attr(docsrs, doc(cfg(feature = "bdev")))]

mod any;
mod descriptor;
mod dif;
mod io;
mod io_channel;
mod owned;

use std::{
    alloc::{Layout, LayoutError},
    ffi::CStr,
    fmt::{self, Debug, Formatter},
};

use spdk_sys::{
    SPDK_ENV_NUMA_ID_ANY, spdk_bdev, spdk_bdev_first, spdk_bdev_get_block_size,
    spdk_bdev_get_buf_align, spdk_bdev_get_dif_pi_format, spdk_bdev_get_dif_type,
    spdk_bdev_get_md_size, spdk_bdev_get_name, spdk_bdev_get_num_blocks, spdk_bdev_get_numa_id,
    spdk_bdev_get_optimal_io_boundary, spdk_bdev_get_physical_block_size,
    spdk_bdev_get_product_name, spdk_bdev_get_uuid, spdk_bdev_get_write_unit_size,
    spdk_bdev_has_write_cache, spdk_bdev_io_type_supported, spdk_bdev_is_dif_check_enabled,
    spdk_bdev_is_dif_head_of_md, spdk_bdev_is_md_interleaved, spdk_bdev_is_zoned, spdk_bdev_next,
    spdk_bdev_wait_for_examine,
};

use crate::thread::{Promise, Promissory};

use super::{Result, Uuid, to_poll_pending_on_ok};

pub use any::Any;
pub use descriptor::{Descriptor, EventHandler, EventType};
pub use dif::{
    CheckFlag as DifCheckFlag, CheckType as DifCheckType, PiFormat as DifPiFormat, Type as DifType,
};
pub use io::{IoError, IoResult, IoType};
pub use io_channel::IoChannel;
pub use owned::{Owned, OwnedBy, OwnedOps};

#[cfg(feature = "bdev-aio")]
#[path = "."]
mod bdev_aio {
    mod aio;

    pub use aio::Aio;
}

#[cfg(feature = "bdev-aio")]
pub use bdev_aio::*;

#[cfg(feature = "bdev-malloc")]
#[path = "."]
mod bdev_malloc {
    mod malloc;

    pub use malloc::{Builder as MallocBuilder, Malloc};
}

#[cfg(feature = "bdev-malloc")]
pub use bdev_malloc::*;

#[cfg(feature = "bdev-module")]
#[path = "."]
mod bdev_module {
    mod bdev_impl;
    mod claim;
    mod io_channel_impl;
    mod io_request;
    mod module;

    pub use bdev_impl::{BDevImpl, BDevOps, Builder as BDevImplBuilder};
    pub use claim::ClaimType;
    pub use io_channel_impl::IoChannelImpl;
    pub use io_request::IoRequest;
    pub use module::{Examiner, Module, ModuleInstance, ModuleOps};
}

#[cfg(feature = "bdev-module")]
pub use bdev_module::*;

#[cfg(feature = "bdev-uring")]
#[path = "."]
mod bdev_uring {
    mod uring;

    pub use uring::{Builder as UringBuilder, Uring};
}

#[cfg(feature = "bdev-uring")]
pub use bdev_uring::*;

/// A trait for block devices providing access to the raw `spdk_bdev` pointer.
pub trait AsRawBDev {
    /// Returns a raw pointer to the underlying `spdk_bdev` structure.
    fn as_raw_bdev(&self) -> *mut spdk_bdev;
}

/// Represents a block device.
///
/// The type `T` determines how the `BDev` manages the underlying `spdk_bdev` lifetime. If `T`
/// implements [`OwnedOps`] it means that the `BDev` is responsible for managing the lifetime of the
/// underlying `spdk_bdev`. The [`BDev::destroy()`] method must be used to explicitly destroy the
/// device when it is no longer needed. `BDev` instances that own the lifetime of the underlying
/// `spdk_bdev` can be shared with other SPDK threads by reference safely and are therefore `Sync`
/// as long as `T` is also `Sync`. Since the `BDev::destroy()` method must be called on the thread
/// on which the block device was created, it is not `Send`.
///
/// If `T` does not implement [`OwnedOps`] (e.g. [`OwnedBy`] and [`Any`]), the `BDev` does not
/// manage the lifetime of the underlying `spdk_bdev` and may be shared by reference or by value
/// across threads safely. It is both `Send` and `Sync` in this case as long as `T` is also `Send`
/// and `Sync`.
#[repr(transparent)]
pub struct BDev<T>(T)
where
    T: AsRawBDev;

unsafe impl<T> Sync for BDev<T> where T: AsRawBDev + Sync {}

impl<T> BDev<T>
where
    T: AsRawBDev,
{
    /// Get an owned [`BDev`] for a block device.
    pub(crate) fn new(dev: T) -> Self {
        Self(dev)
    }

    /// Get an `Any` instance representing this block device.
    pub async fn as_any(&self) -> Result<Any> {
        // SAFETY: The `spdk_bdev` pointer returned by `as_raw_device()` is non-null and valid.
        unsafe { Any::from_ptr_unchecked(self.0.as_raw_bdev()) }
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_raw_bdev()
    }

    /// Opens the device.
    ///
    /// It `write` is `true`, the device is opened for read/write access. Otherwise, it is opened
    /// read-only.
    pub fn open(&self, write: bool) -> Result<Descriptor<'static, ()>> {
        Descriptor::open(self.name(), write)
    }

    /// Opens the device with a custom event handler.
    ///
    /// It `write` is `true`, the device is opened for read/write access. Otherwise, it is opened
    /// read-only.
    pub fn open_with_handler<'a, E, R>(&self, write: bool, handler: R) -> Result<Descriptor<'a, E>>
    where
        E: EventHandler + 'a,
        R: AsRef<E>,
    {
        Descriptor::open_with_handler(self.name(), write, handler)
    }

    /// Opens and claims the block device with the specified claim type and module.
    ///
    /// If the claim type is [`ReadManyWriteNone`], the returned [`Descriptor`] is read-only.
    /// Othwerise, it is read-write.
    ///
    /// [`ReadManyWriteNone`]: ClaimType::ReadManyWriteNone
    #[cfg(feature = "bdev-module")]
    pub fn claim<'a, M, E, R>(
        &self,
        type_: ClaimType<'_>,
        module: &Module<M>,
        handler: R,
    ) -> Result<Descriptor<'a, E>>
    where
        M: ModuleInstance<M> + ModuleOps + 'static,
        E: EventHandler + 'a,
        R: AsRef<E>,
    {
        let desc = self.open_with_handler(false, handler)?;

        desc.claim(type_, module)?;

        Ok(desc)
    }

    /// Get the name of this block device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_bdev_get_name(self.as_ptr())) }
    }

    /// Get the UUID of this block device.
    pub fn uuid(&self) -> Uuid {
        // SAFETY: `spdk_bdev_get_uuid` returns a valid pointer to an `spdk_uuid` associated with
        // this block device.
        unsafe { Uuid::from_ptr_unchecked(spdk_bdev_get_uuid(self.as_ptr())) }
    }

    /// Get the product name of this block device.
    pub fn product_name(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_bdev_get_product_name(self.as_ptr())) }
    }

    /// Get the logical block size of this block device in bytes.
    pub fn logical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_block_size(self.as_ptr()) }
    }

    /// Get the number of logical blocks of this block device.
    pub fn logical_block_count(&self) -> u64 {
        unsafe { spdk_bdev_get_num_blocks(self.as_ptr()) }
    }

    /// Get the physical block size of this block device in bytes.
    pub fn physical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_physical_block_size(self.as_ptr()) }
    }

    /// Get the write unit size of this block device in logical blocks.
    ///
    /// This is the minimum number of blocks that can be written in a single operation. Write
    /// operations must be a multiple of the write unit size.
    pub fn write_unit_size(&self) -> u32 {
        unsafe { spdk_bdev_get_write_unit_size(self.as_ptr()) }
    }

    /// Get the optimal I/O boundary of this block device in logical blocks.
    ///
    /// This is the optimal boundary in logical blocks that should not be crosseed for best
    /// performance. This function returns `0` if there is no optimal I/O boundary.
    pub fn optimal_io_boundary(&self) -> u32 {
        unsafe { spdk_bdev_get_optimal_io_boundary(self.as_ptr()) }
    }

    /// Get the minimum I/O buffer alignment, in bytes, of this block device.
    pub fn buffer_alignment(&self) -> usize {
        unsafe { spdk_bdev_get_buf_align(self.as_ptr()) }
    }

    /// Get whether the metadata of this block device is interleaved with or separated from the
    /// block data.
    ///
    /// The returned value if only meaningful if the metadata size is non-zero.
    pub fn is_metadata_interleaved(&self) -> bool {
        unsafe { spdk_bdev_is_md_interleaved(self.as_ptr()) }
    }

    /// Get the size of the metadata of this block device in bytes.
    ///
    /// A return value of zero indicates that this block device does not have metadata.
    pub fn metadata_size(&self) -> u32 {
        unsafe { spdk_bdev_get_md_size(self.as_ptr()) }
    }

    /// Get the [Data Integrity Field (DIF)] type of this block device.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn dif_type(&self) -> DifType {
        unsafe { spdk_bdev_get_dif_type(self.as_ptr()).into() }
    }

    /// Get the [Data Integrity Field (DIF)] protection information format of this block device.
    ///
    /// # Returns
    ///
    /// Returns `Some(pi)` if DIF is enabled and `None` otherwise.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn dif_pi_format(&self) -> Option<DifPiFormat> {
        if self.dif_type() != DifType::Disabled {
            return Some(unsafe {
                spdk_bdev_get_dif_pi_format(self.as_ptr())
                    .try_into()
                    .expect("valid PI format")
            });
        }

        None
    }

    /// Get whether the specified [Data Integrity Field (DIF)] check is enabled.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn is_dif_check_enabled(&self, check_type: DifCheckType) -> bool {
        unsafe { spdk_bdev_is_dif_check_enabled(self.as_ptr(), check_type.into()) }
    }

    /// Get the bitmap of enabled [Data Integrity Field (DIF)] checks.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn dif_check_flags(&self) -> DifCheckFlag {
        DifCheckFlag::from_bits_truncate(unsafe { (*self.as_ptr()).dif_check_flags })
    }

    /// Get whether the [Data Integrity Field (DIF)] is set in the first 8|16 bytes or last 8|16
    /// bytes of metadata.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn is_dif_head_of_metadata(&self) -> bool {
        unsafe { spdk_bdev_is_dif_head_of_md(self.as_ptr()) }
    }

    /// Get the NUMA node ID of this block device.
    ///
    /// # Returns
    ///
    /// The `Some(node_id)` or `None` if the ID is not known.
    pub fn numa_id(&self) -> Option<i32> {
        let node_id = unsafe { spdk_bdev_get_numa_id(self.as_ptr()) };

        if node_id != SPDK_ENV_NUMA_ID_ANY {
            return Some(node_id);
        }

        None
    }

    /// Get the [`Layout`] for a buffer of the specified byte size.
    pub fn layout_for_size(&self, size: usize) -> std::result::Result<Layout, LayoutError> {
        Layout::from_size_align(size, self.buffer_alignment())
    }

    /// Get the [`Layout`] for a buffer of the specified number of logical blocks.
    pub fn layout_for_blocks(&self, count: u64) -> std::result::Result<Layout, LayoutError> {
        self.layout_for_size(count as usize * self.logical_block_size() as usize)
    }

    /// Gets whether this block device supports zoned namespace semantics.
    pub fn is_zoned(&self) -> bool {
        unsafe { spdk_bdev_is_zoned(self.as_ptr()) }
    }

    /// Gets whether this block device has an enabled write cache.
    pub fn has_write_cache(&self) -> bool {
        unsafe { spdk_bdev_has_write_cache(self.as_ptr()) }
    }

    /// Gets whether this block device supports the specified I/O type.
    pub fn io_type_supported(&self, io_type: IoType) -> bool {
        unsafe { spdk_bdev_io_type_supported(self.as_ptr(), io_type.into()) }
    }
}

impl<T> BDev<T>
where
    T: OwnedOps + From<Owned>,
{
    /// Returns a type-erased [`BDev<Owned>`] instance assuming ownership of the underlying
    /// `spdk_bdev` pointer. This `BDev` is consumed in the process.
    pub fn into_owned(self) -> BDev<Owned> {
        // SAFETY: The `spdk_bdev` pointer is guaranteed to be valid and non-null, and this `BDev`
        // instance, the sole onwer of it, is consumed in the conversion.
        unsafe { Owned::new(self.0) }
    }

    /// Destroy an owned block device asynchronously.
    pub async fn destroy(self) -> Result<()> {
        self.0.destroy().await
    }
}

impl<T> Debug for BDev<T>
where
    T: AsRawBDev,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Device({:?})", self.name().to_string_lossy())
    }
}

impl<'a, O> BDev<OwnedBy<'a, O>> {
    /// Gets a borrowed block device from an object that logically owns the device while it is in
    /// use, e.g. [`IoRequest`] & [`IoChannel`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bdev` is a non-null pointer to an `spdk_bdev` structure whose
    /// lifetime is tied to the owner.
    ///
    /// [`IoRequest`]: crate::bdev::IoRequest
    /// [`IoChannel`]: crate::bdev::IoChannel
    pub(crate) unsafe fn with_owner(_owner: &'a O, bdev: *mut spdk_bdev) -> BDev<OwnedBy<'a, O>> {
        BDev::<OwnedBy<'a, O>>(unsafe { OwnedBy::new_unchecked(bdev) })
    }
}

/// Since `BDev<OwnedBy<'_, O>>` does not manage the underlying `spdk_bdev`'s lifetime, it is safe
/// to send it across threads.
unsafe impl<O> Send for BDev<OwnedBy<'_, O>> {}

impl BDev<Any> {
    /// Get a [`BDev`] by its name.
    ///
    /// A `BDev<Any>` instance is a non-owning reference to the underlying `spdk_bdev` instance. It
    /// ensures that the underlying `spdk_bdev` remains valid for the lifetime of the `BDev<Any>`
    /// instance but cannot destroy it. However, a call to [`BDev::destroy()`] by another task will
    /// be deferred until all `BDev<Any>` instances referencing the same `spdk_bdev` are dropped.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(dev)` if the device exists and `Err(`[`ENODEV`]`)` if no block
    /// device with the given name exists. It may return other errors if initialization of the
    /// `BDev<Any>` instance fails. See [`Any`] for details.
    ///
    /// [`ENODEV`]: crate::errors::ENODEV
    pub fn from_name(name: &CStr) -> Result<BDev<Any>> {
        Any::with_name(name).map(BDev::<Any>)
    }

    /// Get a [`BDev`] for a raw `spdk_bdev` pointer.
    ///
    /// A `BDev<Any>` instance is a non-owning reference to the underlying `spdk_bdev` instance. It
    /// ensures that the underlying `spdk_bdev` remains valid for the lifetime of the `BDev<Any>`
    /// instance but cannot destroy it. However, a call to [`BDev::destroy()`] by another task will
    /// be deferred until all `BDev<Any>` instances referencing the same `spdk_bdev` are dropped.
    ///
    /// # Returns
    ///
    /// This method may return an error if initialization of the `BDev<Any>` instance fails. See
    /// [`Any`] for details.
    ///
    /// # Safety
    ///
    /// `bdev` must be non-null and a pointer to a valid `spdk_bdev` structure.
    pub(crate) unsafe fn from_ptr_unchecked(bdev: *mut spdk_bdev) -> Result<BDev<Any>> {
        unsafe { Any::from_ptr_unchecked(bdev).map(BDev::<Any>) }
    }
}

/// Since `BDev<Any>` does not manage the underlying `spdk_bdev`'s lifetime, it is safe to send it
/// across threads.
unsafe impl Send for BDev<Any> {}

/// An iterator over all block devices.
pub struct BDevs {
    next: Option<BDev<Any>>,
}

impl BDevs {
    /// Creates a new iterator over all block devices.
    fn new() -> Self {
        Self {
            next: Self::get_device(unsafe { spdk_bdev_first() }),
        }
    }

    /// Gets the next available block device starting from the given 'spdk_bdev' pointer.
    fn get_device(mut bdev: *mut spdk_bdev) -> Option<BDev<Any>> {
        while !bdev.is_null() {
            if let Ok(device) = unsafe { BDev::from_ptr_unchecked(bdev) } {
                return Some(device);
            }

            bdev = unsafe { spdk_bdev_next(bdev) };
        }

        None
    }
}

impl Default for BDevs {
    fn default() -> Self {
        BDevs::new()
    }
}

impl Iterator for BDevs {
    type Item = BDev<Any>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.next.take() {
            self.next = Self::get_device(unsafe { spdk_bdev_next(next.as_ptr()) });

            return Some(next);
        }

        None
    }
}

/// Get an iterator over all block devices.
///
/// # Example
///
/// ```no_run
#[doc = include_str!("../../examples/bdev_all.rs")]
/// ```
pub fn all() -> BDevs {
    BDevs::new()
}

/// Waits for the examination process to finish on all block devices.
pub async fn wait_for_examination() -> Result<()> {
    Promise::new()
        .request(|p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_ok(p);

            to_poll_pending_on_ok! {
                unsafe { spdk_bdev_wait_for_examine(Some(cb_fn), cb_arg as *const _ as *mut _) }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
            .map_err(Into::into)
        })
        .await
}
