use std::{
    alloc::{Layout, LayoutError},
    ffi::CStr,
    fmt::{self, Debug, Formatter},
    future::Future,
    mem,
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll},
};

use futures::{FutureExt, Stream};
use spdk_sys::{
    SPDK_BDEV_IO_TYPE_ABORT, SPDK_BDEV_IO_TYPE_COMPARE, SPDK_BDEV_IO_TYPE_COMPARE_AND_WRITE,
    SPDK_BDEV_IO_TYPE_COPY, SPDK_BDEV_IO_TYPE_FLUSH, SPDK_BDEV_IO_TYPE_GET_ZONE_INFO,
    SPDK_BDEV_IO_TYPE_INVALID, SPDK_BDEV_IO_TYPE_NVME_ADMIN, SPDK_BDEV_IO_TYPE_NVME_IO,
    SPDK_BDEV_IO_TYPE_NVME_IO_MD, SPDK_BDEV_IO_TYPE_NVME_IOV_MD, SPDK_BDEV_IO_TYPE_NVME_NSSR,
    SPDK_BDEV_IO_TYPE_READ, SPDK_BDEV_IO_TYPE_RESET, SPDK_BDEV_IO_TYPE_SEEK_DATA,
    SPDK_BDEV_IO_TYPE_SEEK_HOLE, SPDK_BDEV_IO_TYPE_UNMAP, SPDK_BDEV_IO_TYPE_WRITE,
    SPDK_BDEV_IO_TYPE_WRITE_UNCORRECTABLE, SPDK_BDEV_IO_TYPE_WRITE_ZEROES, SPDK_BDEV_IO_TYPE_ZCOPY,
    SPDK_BDEV_IO_TYPE_ZONE_APPEND, SPDK_BDEV_IO_TYPE_ZONE_MANAGEMENT, SPDK_ENV_NUMA_ID_ANY,
    spdk_bdev, spdk_bdev_first, spdk_bdev_get_block_size, spdk_bdev_get_buf_align,
    spdk_bdev_get_by_name, spdk_bdev_get_dif_pi_format, spdk_bdev_get_dif_type,
    spdk_bdev_get_md_size, spdk_bdev_get_name, spdk_bdev_get_num_blocks, spdk_bdev_get_numa_id,
    spdk_bdev_get_optimal_io_boundary, spdk_bdev_get_physical_block_size,
    spdk_bdev_get_product_name, spdk_bdev_get_write_unit_size, spdk_bdev_has_write_cache,
    spdk_bdev_io_type, spdk_bdev_io_type_supported, spdk_bdev_is_dif_check_enabled,
    spdk_bdev_is_dif_head_of_md, spdk_bdev_is_md_interleaved, spdk_bdev_is_zoned, spdk_bdev_next,
    spdk_dif_check_type, spdk_dif_pi_format,
    spdk_dif_type::{self, SPDK_DIF_DISABLE},
};

use crate::{
    Result,
    block::{Any, Owned, OwnedOps},
    errors::{ENODEV, EPERM},
    thread,
};

use super::Descriptor;

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
    NvmeIovMd,
    NvmeNssr,
    WriteUncorrectable,
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
            SPDK_BDEV_IO_TYPE_NVME_IOV_MD => IoType::NvmeIovMd,
            SPDK_BDEV_IO_TYPE_NVME_NSSR => IoType::NvmeNssr,
            SPDK_BDEV_IO_TYPE_WRITE_UNCORRECTABLE => IoType::WriteUncorrectable,
            _ => unreachable!("unexpected spdk_bdev_io_type value"),
        }
    }
}

/// Represents the ownership state of a [`Device`].
enum OwnershipState<T: OwnedOps> {
    Owned(T),
    Borrowed(NonNull<spdk_bdev>),
    None,
}

unsafe impl<T: OwnedOps> Send for OwnershipState<T> {}

/// Represents a block device.
///
/// `Device` wraps an `spdk_bdev` pointer and can be in one of three ownership states: owned,
/// borrowed, or none.
///
/// An owned device owns the underlying `spdk_bdev` pointer and will destroy it when dropped. The
/// caller must ensure that the drop occurs in the same thread that created the device. It must also
/// occur as part of thread event handling by explicitly calling [`task::yield_now`] before dropping
/// the device. However, it is easiest and safest to explicitly call [`Device<T>::destroy`] on the
/// device rather than let it drop naturally.
///
/// A borrowed device borrows the underlying `spdk_bdev` pointer. Dropping a borrowed device has no
/// effect on the underlying `spdk_bdev` pointer.
///
/// A device with no ownership state can only be safely queried for ownership state or dropped. Any
/// other operation will panic. A device will be left in this state after the [`Device<T>::take`]
/// method is called.
///
/// [`Device<T>::destroy`]: method@Device<T>::destroy
/// [`Device<T>::take`]: method@Device<T>::take
/// [`task::yield_now`]: function@crate::task::yield_now
pub struct Device<T: OwnedOps>(OwnershipState<T>);

unsafe impl<T: OwnedOps> Send for Device<T> {}
unsafe impl<T: OwnedOps> Sync for Device<T> {}

impl<T: OwnedOps> Device<T> {
    /// Get an owned [`Device`] for a block device.
    pub fn new(dev: T) -> Self {
        Self(OwnershipState::Owned(dev))
    }

    /// Get a borrowed [`Device`] by its name.
    ///
    /// # Returns
    ///
    /// This function returns [`None`] if no block device with the given name exists.
    pub fn from_name(name: &CStr) -> Option<Device<Any>> {
        let bdev = unsafe { spdk_bdev_get_by_name(name.as_ptr()) };

        NonNull::new(bdev).map(|b| Device::<Any>(OwnershipState::Borrowed(b)))
    }

    /// Attempt to get a borrowed [`Device`] for a raw `spdk_bdev` pointer.
    ///
    /// # Returns
    ///
    /// Returns `Some(dev)` if `bdev` is non-null and `None` otherwise.
    pub fn try_from_ptr(bdev: *mut spdk_bdev) -> Option<Device<Any>> {
        NonNull::new(bdev).map(|b| Device::<Any>(OwnershipState::Borrowed(b)))
    }

    /// Get a borrowed [`Device`] for a raw `spdk_bdev` pointer.
    ///
    /// # Panics
    ///
    /// This method panics if `bdev` is null.
    pub fn from_ptr(bdev: *mut spdk_bdev) -> Device<Any> {
        Self::try_from_ptr(bdev).expect("device pointer must not be null")
    }

    /// Get a borrowed [`Device`] for a raw `spdk_bdev` pointer.
    ///
    /// # Safety
    ///
    /// `bdev` must be non-null.
    pub unsafe fn from_ptr_unchecked(bdev: *mut spdk_bdev) -> Device<Any> {
        Device::<Any>(OwnershipState::Borrowed(unsafe {
            NonNull::new_unchecked(bdev)
        }))
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    ///
    /// # Panics
    ///
    /// This method panics if this device has no ownership state.
    pub fn as_ptr(&self) -> *mut spdk_bdev {
        match &self.0 {
            OwnershipState::Owned(dev) => dev.as_ptr(),
            OwnershipState::Borrowed(bdev) => bdev.as_ptr(),
            _ => panic!("no device"),
        }
    }

    /// Consumes this device and returns a [`Device<Owned>`] assuming ownership of the underlying
    /// `spdk_bdev` pointer.
    pub fn into_owned(&mut self) -> Option<Device<Owned>> {
        match self.0 {
            OwnershipState::Owned(_) => match mem::replace(&mut self.0, OwnershipState::None) {
                OwnershipState::Owned(dev) => Some(Owned::new(dev)),
                _ => unreachable!(),
            },
            _ => None,
        }
    }

    /// Borrow this device.
    pub fn borrow(&self) -> Device<Any> {
        match &self.0 {
            OwnershipState::Owned(dev) => Device::<Any>(OwnershipState::Borrowed(unsafe {
                NonNull::new_unchecked(dev.as_ptr())
            })),
            OwnershipState::Borrowed(bdev) => Device::<Any>(OwnershipState::Borrowed(*bdev)),
            OwnershipState::None => panic!("no device"),
        }
    }

    /// Returns whether this device is owned.
    pub fn is_owned(&self) -> bool {
        matches!(self.0, OwnershipState::Owned(_))
    }

    /// Returns whether this device is borrowed.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.0, OwnershipState::Borrowed(_))
    }

    /// Returns whether this device has no ownership state.
    pub fn is_none(&self) -> bool {
        matches!(self.0, OwnershipState::None)
    }

    /// Takes the value from this device and replaces with a value having no ownership.
    pub fn take(&mut self) -> Self {
        mem::replace(self, Self(OwnershipState::None))
    }

    /// Destroy the block device asynchronously.
    ///
    /// # Returns
    ///
    /// Only an owned device can be destroyed. This function returns `Err(EPERM)` if called on a
    /// borrowed device and `Err(ENODEV)` if called on a device that neither owns nor borrows the
    /// underlying `spdk_bdev` pointer.
    pub async fn destroy(mut self) -> Result<()> {
        match self.0 {
            OwnershipState::Borrowed(_) => Err(EPERM),
            OwnershipState::None => Err(ENODEV),
            OwnershipState::Owned(_) => match mem::replace(&mut self.0, OwnershipState::None) {
                OwnershipState::Owned(dev) => dev.destroy().await,
                _ => unreachable!(),
            },
        }
    }

    /// Opens the device asynchronously.
    pub async fn open(&self, write: bool) -> Result<Descriptor> {
        Descriptor::open(self.name(), write).await
    }

    /// Get the name of this block device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr(spdk_bdev_get_name(self.as_ptr())) }
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
    pub fn dif_type(&self) -> spdk_dif_type {
        unsafe { spdk_bdev_get_dif_type(self.as_ptr()) }
    }

    /// Get the [Data Integrity Field (DIF)] protection information format of this block device.
    ///
    /// # Returns
    ///
    /// Returns `Some(pi)` if DIF is enabled and `None` otherwise.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn dif_pi_format(&self) -> Option<spdk_dif_pi_format> {
        if self.dif_type() != SPDK_DIF_DISABLE {
            return Some(unsafe { spdk_bdev_get_dif_pi_format(self.as_ptr()) });
        }

        None
    }

    /// Get whether the specified [Data Integrity Field (DIF)] check is enabled.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn is_dif_check_enabled(&self, check_type: spdk_dif_check_type) -> bool {
        unsafe { spdk_bdev_is_dif_check_enabled(self.as_ptr(), check_type) }
    }

    /// Get the bitmap of enabled [Data Integrity Field (DIF)] checks.
    ///
    /// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
    pub fn dif_check_flags(&self) -> u32 {
        unsafe { (*self.as_ptr()).dif_check_flags }
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
        unsafe { spdk_bdev_io_type_supported(self.as_ptr(), io_type as u32) }
    }

    /// Gets the first `BDev` in the global list.
    ///
    /// Returns `None` if there are no `BDev`s currently registered.
    fn first() -> Option<Device<Any>> {
        Self::try_from_ptr(unsafe { spdk_bdev_first() })
    }

    /// Gets the next `BDev` in the global list.
    ///
    /// Returns `None` if there are no more `BDev`s in the list.
    fn next(&self) -> Option<Device<Any>> {
        Self::try_from_ptr(unsafe { spdk_bdev_next(self.as_ptr()) })
    }
}

impl<T: OwnedOps> Drop for Device<T> {
    fn drop(&mut self) {
        if self.is_owned() {
            let dev = self.take();

            thread::block_on(async move { dev.destroy().await }).unwrap();
        }
    }
}

impl<T: OwnedOps> Debug for Device<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "Device({})", self.name().to_string_lossy())
    }
}

impl From<*mut spdk_bdev> for Device<Any> {
    fn from(bdev: *mut spdk_bdev) -> Self {
        Device::<Any>::from_ptr(bdev)
    }
}

/// An asynchronous iterator over all block devices.
pub struct Devices {
    current: Option<Device<Any>>,
    desc: Option<Descriptor>,
    open_fut: Option<Pin<Box<dyn Future<Output = Result<Descriptor>>>>>,
}

impl Stream for Devices {
    type Item = Device<Any>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        while let Some(dev) = &self.current {
            let dev = dev.borrow();

            // Open a read-only descriptor on the block device to prevent it
            // from being unregistered (and deleted) while it is in use by the
            // caller. Since this is an asynchronous operation, we store a
            // future to the open call and delegate polling for a result to it.
            if self.open_fut.is_none() {
                let dev = dev.borrow();

                self.open_fut = Some(async move { dev.open(false).await }.boxed_local())
            }

            // Poll the block device open future and process the result.
            match self.open_fut.as_mut().unwrap().poll_unpin(cx) {
                // The device was opened successfully. Store the descriptor,
                // advance the iterator to the next device and return the
                // current device to the caller.
                Poll::Ready(Ok(desc)) => {
                    self.current = dev.next();
                    self.desc = Some(desc);
                    self.open_fut = None;

                    return Poll::Ready(Some(dev.borrow()));
                }

                // An error occurred during the open: skip this device and
                // advance the iterator to the next one.
                Poll::Ready(Err(_)) => {
                    self.current = dev.next();
                    self.desc = None;
                    self.open_fut = None;
                    continue;
                }

                // The open operation is pending.
                Poll::Pending => return Poll::Pending,
            }
        }

        // There are no more devices to be iterated. Close the current
        // descriptor and return `None` to end the iteration.
        self.desc = None;

        Poll::Ready(None)
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
    Devices {
        current: Device::<Any>::first(),
        desc: None,
        open_fut: None,
    }
}
