use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    ffi::CStr,
};

use spdk_sys::{
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

/// Represents a block device.
pub struct Device(pub(crate) *mut spdk_bdev);

unsafe impl Send for Device {}

impl Device {
    /// Get a [`Device`] by its name.
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
            Some(Device(bdev))
        }
    }

    /// Get a [`Device`] for an `spdk_bdev`.
    #[cfg(feature = "bdev")]
    pub(crate) fn from_bdev(bdev: *mut spdk_bdev) -> Self {
        Self(bdev)
    }

    /// Get a pointer to the underlying `spdk_bdev` struct.
    pub fn as_ptr(&self) -> *mut spdk_bdev {
        self.0
    }

    /// Get the name of this block device.
    pub fn name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_name(self.0)) }
    }

    /// Get the product name of this block device.
    pub fn product_name(&self) -> &CStr {
        unsafe { CStr::from_ptr( spdk_bdev_get_product_name(self.0)) }
    }

    /// Get the logical block size of this block device in bytes.
    pub fn logical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_block_size(self.0) }
    }

    /// Get the number of logical blocks of this block device.
    pub fn logical_block_count(&self) -> u64 {
        unsafe { spdk_bdev_get_num_blocks(self.0) }
    }

    /// Get the physical block size of this block device in bytes.
    pub fn physical_block_size(&self) -> u32 {
        unsafe { spdk_bdev_get_physical_block_size(self.0) }
    }

    /// Get the write unit size of this block device in logical blocks.
    ///
    /// This is the minimum number of blocks that can be written in a single
    /// operation. Write operations must be a multiple of the write unit size.
    pub fn write_unit_size(&self) -> u32 {
        unsafe { spdk_bdev_get_write_unit_size(self.0) }
    }

    /// Get the optimal I/O boundary of this block device in logical blocks.
    /// 
    /// This is the optimal boundary in logical blocks that should not be
    /// crosseed for best performance. This function returns `0` if there is
    /// no optimal I/O boundary.
    pub fn optimal_io_boundary(&self) -> u32 {
        unsafe { spdk_bdev_get_optimal_io_boundary(self.0) }
    }

    /// Get the minimum I/O buffer alignment, in bytes, of this block device.
    pub fn buffer_alignment(&self) -> usize {
        unsafe { spdk_bdev_get_buf_align(self.0) }
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
        unsafe { spdk_bdev_is_zoned(self.0) }
    }

    /// Gets whether this block device has an enabled write cache.
    pub fn has_write_cache(&self) -> bool {
        unsafe { spdk_bdev_has_write_cache(self.0) }
    }
}


/// An iterator over all block devices.
pub struct Devices(*mut spdk_bdev);

unsafe impl Send for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_null() {
            None
        } else {
            let current = self.0;

            self.0 = unsafe { spdk_bdev_next(self.0) };

            Some(Device(current))
        }
    }
}

/// Get an iterator over all block devices.
pub fn devices() -> Devices {
    Devices(unsafe { spdk_bdev_first() })
}
