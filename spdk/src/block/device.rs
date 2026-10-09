use std::{
    alloc::{Layout, LayoutError},
    ffi::CStr,
    fmt::{self, Debug, Formatter},
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use spdk_sys::{
    SPDK_ENV_NUMA_ID_ANY, spdk_bdev, spdk_bdev_first, spdk_bdev_get_block_size,
    spdk_bdev_get_buf_align, spdk_bdev_get_dif_pi_format, spdk_bdev_get_dif_type,
    spdk_bdev_get_md_size, spdk_bdev_get_name, spdk_bdev_get_num_blocks, spdk_bdev_get_numa_id,
    spdk_bdev_get_optimal_io_boundary, spdk_bdev_get_physical_block_size,
    spdk_bdev_get_product_name, spdk_bdev_get_uuid, spdk_bdev_get_write_unit_size,
    spdk_bdev_has_write_cache, spdk_bdev_io_type_supported, spdk_bdev_is_dif_check_enabled,
    spdk_bdev_is_dif_head_of_md, spdk_bdev_is_md_interleaved, spdk_bdev_is_zoned, spdk_bdev_next,
};

#[cfg(feature = "bdev-module")]
use ::spdk_sys::spdk_bdev_module_claim_bdev_desc;

use crate::{Result, Uuid};

#[cfg(feature = "bdev-module")]
use crate::{
    bdev::{Module, ModuleInstance, ModuleOps},
    to_result,
};

use super::{
    Any, Descriptor, DifCheckFlag, DifCheckType, DifPiFormat, DifType, IoType, Owned, OwnedBy,
    OwnedOps,
};

#[cfg(feature = "bdev-module")]
use super::ClaimType;

/// A trait for block devices providing access to the raw `spdk_bdev` pointer.
pub trait AsRawBDev {
    /// Returns a raw pointer to the underlying `spdk_bdev` structure.
    fn as_raw_bdev(&self) -> *mut spdk_bdev;
}

/// Represents a block device.
///
/// The type `T` determines how the `Device` manages the underlying `spdk_bdev` lifetime. If `T`
/// implements [`OwnedOps`] it means that the `Device` is responsible for managing the lifetime of
/// the underlying `spdk_bdev`. The [`Device::destroy()`] method must be used to explicitly destroy
/// the device when it is no longer needed. `Device` instances that own the lifetime of the
/// underlying `spdk_bdev` can be shared with other SPDK threads by reference safely and are
/// therefore `Sync` as long as `T` is also `Sync`. Since the `Device::destroy()` method must be
/// called on the thread on which the block device was created, it is not `Send`.
///
/// If `T` does not implement [`OwnedOps`] (e.g. [`OwnedBy`] and [`Any`]), the `Device` does not
/// manage the lifetime of the underlying `spdk_bdev` and may be shared by reference or by value
/// across threads safely. It is both `Send` and `Sync` in this case as long as `T` is also `Send`
/// and `Sync`.
#[repr(transparent)]
pub struct Device<T>(T)
where
    T: AsRawBDev;

unsafe impl<T> Sync for Device<T> where T: AsRawBDev + Sync {}

impl<T> Device<T>
where
    T: AsRawBDev,
{
    /// Get an owned [`Device`] for a block device.
    pub(crate) fn new(dev: T) -> Self {
        Self(dev)
    }

    /// Get an `Any` instance representing this block device.
    pub async fn as_any(&self) -> Result<Any> {
        // SAFETY: The `spdk_bdev` pointer returned by `as_raw_device()`is non-null and valid.
        unsafe { Any::from_ptr_unchecked(self.0.as_raw_bdev()).await }
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_raw_bdev()
    }

    /// Opens the device asynchronously.
    pub async fn open(&self, write: bool) -> Result<Descriptor> {
        Descriptor::open(self.name(), write).await
    }

    /// Claims the block device with the specified claim type and module, returning an open [`Descriptor`].
    ///
    /// If the claim type is [`ReadManyWriteNone`], the returned descriptor is read-only. Othwerise,
    /// it is read-write.
    ///
    /// [`ReadManyWriteNone`]: ClaimType::ReadManyWriteNone
    #[cfg(feature = "bdev-module")]
    pub async fn claim<M>(&self, type_: ClaimType<'_>, module: &Module<M>) -> Result<Descriptor>
    where
        M: ModuleInstance<M> + ModuleOps + 'static,
    {
        let (claim_type, mut opts) = type_.into_params()?;

        let desc = self.open(false).await?;

        unsafe {
            to_result!(spdk_bdev_module_claim_bdev_desc(
                desc.as_ptr(),
                claim_type,
                &mut opts as *mut _,
                module.as_ptr()
            ))
        }?;

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

impl<T> Device<T>
where
    T: OwnedOps + From<Owned>,
{
    /// Returns a type-erased [`Device<Owned>`] instance assuming ownership of the underlying
    /// `spdk_bdev` pointer. This `Device` is consumed in the process.
    pub fn into_owned(self) -> Device<Owned> {
        // SAFETY: The `spdk_bdev` pointer is guaranteed to be valid and non-null, and this `Device`
        // instance, the sole onwer of it, is consumed in the conversion.
        unsafe { Owned::new(self.0) }
    }

    /// Destroy an owned block device asynchronously.
    pub async fn destroy(self) -> Result<()> {
        self.0.destroy().await
    }
}

impl<T> Debug for Device<T>
where
    T: AsRawBDev,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Device({:?})", self.name().to_string_lossy())
    }
}

impl<'a, O> Device<OwnedBy<'a, O>> {
    /// Gets a borrowed block device from an object that logically owns the device while it is in
    /// use, e.g. [`BDevIo`] & [`IoChannel`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bdev` is a non-null pointer to an `spdk_bdev` structure whose
    /// lifetime is tied to the owner.
    ///
    /// [`BDevIo`]: crate::bdev::BDevIo
    /// [`IoChannel`]: crate::block::IoChannel
    pub(crate) unsafe fn with_owner(_owner: &'a O, bdev: *mut spdk_bdev) -> Device<OwnedBy<'a, O>> {
        Device::<OwnedBy<'a, O>>(unsafe { OwnedBy::new_unchecked(bdev) })
    }
}

/// Since `Device<OwnedBy<'_, O>>` does not manage the underlying `spdk_bdev`'s lifetime, it is safe
/// to send it across threads.
unsafe impl<O> Send for Device<OwnedBy<'_, O>> {}

impl Device<Any> {
    /// Get a [`Device`] by its name.
    ///
    /// A `Device<Any>` instance is a non-owning reference to the underlying `spdk_bdev` instance.
    /// It ensures that the underlying `spdk_bdev` remains valid for the lifetime of the
    /// `Device<Any>` instance but cannot destroy it. However, a call to [`Device::destroy()`] by
    /// another task will be deferred until all `Device<Any>` instances referencing the same
    /// `spdk_bdev` are dropped.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(dev)` if the device exists and `Err(`[`ENODEV`]`)` if no block device
    /// with the given name exists. It may return other errors if initialization of the
    /// `Device<Any>` instance fails. See [`Any`] for details.
    ///
    /// [`ENODEV`]: crate::errors::ENODEV
    pub async fn from_name(name: &CStr) -> Result<Device<Any>> {
        Any::with_name(name).await.map(Device::<Any>)
    }

    /// Get a [`Device`] for a raw `spdk_bdev` pointer.
    ///
    /// A `Device<Any>` instance is a non-owning reference to the underlying `spdk_bdev` instance.
    /// It ensures that the underlying `spdk_bdev` remains valid for the lifetime of the
    /// `Device<Any>` instance but cannot destry it. However, a call to [`Device::destroy()`] by
    /// another task will be deferred until all `Device<Any>` instances referencing the same
    /// `spdk_bdev` are dropped.
    ///
    /// # Returns
    ///
    /// This method may return an error if initialization of the `Device<Any>` instance fails. See
    /// [`Any`] for details.
    ///
    /// # Safety
    ///
    /// `bdev` must be non-null and a pointer to a valid `spdk_bdev` structure.
    pub(crate) async unsafe fn from_ptr_unchecked(bdev: *mut spdk_bdev) -> Result<Device<Any>> {
        unsafe { Any::from_ptr_unchecked(bdev).await.map(Device::<Any>) }
    }
}

/// Since `Device<Any>` does not manage the underlying `spdk_bdev`'s lifetime, it is safe to send it
/// across threads.
unsafe impl Send for Device<Any> {}

type DevicesFuture = Pin<Box<dyn Future<Output = Option<(Device<Any>, Option<Device<Any>>)>>>>;

/// An asynchronous iterator over all block devices.
pub struct Devices {
    next_fut: Option<DevicesFuture>,
}

impl Devices {
    /// Creates a new asynchronous iterator over all block devices.
    fn new() -> Self {
        Self {
            next_fut: Some(Box::pin(async {
                let current = Self::get_device(unsafe { spdk_bdev_first() }).await;

                Self::get_next_state(current).await
            })),
        }
    }

    /// Gets the next available block device starting from the given 'spdk_bdev' pointer.
    async fn get_device(mut bdev: *mut spdk_bdev) -> Option<Device<Any>> {
        while !bdev.is_null() {
            if let Ok(device) = unsafe { Device::from_ptr_unchecked(bdev) }.await {
                return Some(device);
            }

            bdev = unsafe { spdk_bdev_next(bdev) };
        }

        None
    }

    /// Gets the next state in the iteration.
    async fn get_next_state(
        current: Option<Device<Any>>,
    ) -> Option<(Device<Any>, Option<Device<Any>>)> {
        if let Some(current) = current {
            let next_bdev = unsafe { spdk_bdev_next(current.as_ptr()) };
            let next = Self::get_device(next_bdev).await;

            return Some((current, next));
        }

        None
    }
}

impl Default for Devices {
    fn default() -> Self {
        Devices::new()
    }
}

impl Stream for Devices {
    type Item = Device<Any>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next_state = match &mut self.next_fut {
            Some(next_fut) => next_fut.as_mut().poll(cx),
            None => Poll::Ready(None),
        };

        match next_state {
            Poll::Ready(Some((current, next))) => {
                self.next_fut = Some(Box::pin(async move { Self::get_next_state(next).await }));

                Poll::Ready(Some(current))
            }
            Poll::Ready(None) => {
                self.next_fut = None;

                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Get an asynchronous iterator over all block devices.
///
/// # Example
///
/// ```no_run
#[doc = include_str!("../../examples/devices.rs")]
/// ```
pub fn devices() -> Devices {
    Devices::new()
}
