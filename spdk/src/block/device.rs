use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    ffi::CStr,
    marker::PhantomData,
    mem::{self},
};

use spdk_sys::{
    Errno,
    spdk_bdev,

    spdk_bdev_first,
    spdk_bdev_get_block_size,
    spdk_bdev_get_buf_align,
    spdk_bdev_get_by_name,
    spdk_bdev_get_name,
    spdk_bdev_get_num_blocks,
    spdk_bdev_get_optimal_io_boundary,
    spdk_bdev_get_physical_block_size,
    spdk_bdev_get_product_name,
    spdk_bdev_get_write_unit_size,
    spdk_bdev_has_write_cache,
    spdk_bdev_is_zoned,
    spdk_bdev_next,
};

use crate::{
    bdev::{
        Any,
        BDev,
    },
    errors::{
        EPERM,
        ENODEV,
    },
    thread,
};

use super::Descriptor;

/// Represents a block device.
pub enum Device<T: BDev + Send + From<Device<T>> + 'static> {
    Owned(*mut spdk_bdev, PhantomData<T>),
    Borrowed(*mut spdk_bdev),
    None,
}

unsafe impl <T: BDev + Send + From<Device<T>> + 'static> Send for Device<T> {}
unsafe impl <T: BDev + Send + From<Device<T>> + 'static> Sync for Device<T> {}

impl <T: BDev + Send + From<Device<T>> + 'static> Device<T> {
    /// Get a borrowed [`Device`] by its name.
    /// 
    /// # Returns
    /// 
    /// This function returns [`None`] if no block device with the given name
    /// exists.
    pub fn from_name(name: &CStr) -> Option<Self> {
        let bdev = unsafe { spdk_bdev_get_by_name(name.as_ptr()) };

        if bdev.is_null() {
            None
        } else {
            Some(Self::Borrowed(bdev))
        }
    }

    /// Get a borrowed [`Device`] for a raw `spdk_bdev` pointer.
    pub(crate) fn from_ptr(bdev: *mut spdk_bdev) -> Self {
        Self::Borrowed(bdev)
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    /// 
    /// # Panics
    /// 
    /// This method panics if this device is [`Device::None`].
    pub(crate) fn as_ptr(&self) -> *mut spdk_bdev {
        match self {
            Self::Owned(bdev, _) | Self::Borrowed(bdev) => *bdev,
            _ => panic!("no device"),
        }
    }

    /// Borrow this device.
    pub fn borrow(&self) -> Self {
        Self::Borrowed(self.as_ptr())
    }

    /// Returns whether this device is owned.
    pub fn is_owned(&self) -> bool {
        matches!(self, Self::Owned(_, _))
    }

    /// Returns whether this device is borrowed.
    pub fn is_borrowed(&self) -> bool {
        matches!(self, Self::Borrowed(_))
    }

    /// Returns whether this device is [`Device::None`].
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Takes the value from this device and replaces it with [`Device::None`].
    pub fn take(&mut self) -> Self {
        mem::replace(self, Self::None)
    }

    /// Destroy the block device asynchronously.
    /// 
    /// # Returns
    /// 
    /// Only an owned device can be destroyed. This function returns
    /// [`Err(EPERM)`] if called on a borrowed device and [`Err(ENODEV)`] if
    /// called on [`Device::None`].
    pub async fn destroy(mut self) -> Result<(), Errno> {
        match self {
            Self::Borrowed(_) => Err(EPERM),
            Self::None => Err(ENODEV),
            Self::Owned(_, _) => {
                T::destroy(self.take().into()).await
            },
        }
    }

    /// Opens the device asynchronously.
    pub async fn open(&self, write: bool) -> Result<Descriptor, Errno> {
        Descriptor::open(self.name(), write).await
    }
    
    /// Get the name of this block device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_name(self.as_ptr())) }
    }

    /// Get the product name of this block device.
    pub fn product_name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_product_name(self.as_ptr())) }
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
    /// This is the minimum number of blocks that can be written in a single
    /// operation. Write operations must be a multiple of the write unit size.
    pub fn write_unit_size(&self) -> u32 {
        unsafe { spdk_bdev_get_write_unit_size(self.as_ptr()) }
    }

    /// Get the optimal I/O boundary of this block device in logical blocks.
    /// 
    /// This is the optimal boundary in logical blocks that should not be
    /// crosseed for best performance. This function returns `0` if there is
    /// no optimal I/O boundary.
    pub fn optimal_io_boundary(&self) -> u32 {
        unsafe { spdk_bdev_get_optimal_io_boundary(self.as_ptr()) }
    }

    /// Get the minimum I/O buffer alignment, in bytes, of this block device.
    pub fn buffer_alignment(&self) -> usize {
        unsafe { spdk_bdev_get_buf_align(self.as_ptr()) }
    }

    /// Get the [`Layout`] for a buffer of the specified byte size.
    pub fn layout_for_size(&self, size: usize) -> Result<Layout, LayoutError> {
        Layout::from_size_align(size, self.buffer_alignment())
    }

    /// Get the [`Layout`] for a buffer of the specified number of logical
    /// blocks.
    pub fn layout_for_blocks(&self, count: u64) -> Result<Layout, LayoutError> {
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
}

impl <T: BDev + Send + From<Device<T>> + 'static> Drop for Device<T> {
    fn drop(&mut self) {
        if self.is_owned() {
            let dev = self.take();

            thread::block_on(async move { dev.destroy().await }).unwrap();
        }
    }
}

/// An iterator over all block devices.
pub struct Devices(*mut spdk_bdev);

unsafe impl Send for Devices {}

impl Iterator for Devices {
    type Item = Device<Any>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            None
        } else {
            let current = self.0;

            self.0 = unsafe { spdk_bdev_next(self.0) };

            Some(Device::Borrowed(current))
        }
    }
}

/// Get an iterator over all block devices.
pub fn devices() -> Devices {
    Devices(unsafe { spdk_bdev_first() })
}
