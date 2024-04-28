use std::{
    alloc::{
        Layout,
        LayoutError,
    },
    ffi::CStr,
    mem::{self},
    ptr::NonNull,
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

use crate::{
    block::{
        Any,
        OwnedOps,
        Owned,
    },
    errors::{
        Errno,

        EPERM,
        ENODEV,
    },
    thread,
};

use super::Descriptor;

/// Represents the ownership state of a [`Device`].
enum OwnershipState<T: OwnedOps + Send + 'static> {
    Owned(T),
    Borrowed(NonNull<spdk_bdev>),
    None,
}

unsafe impl<T: OwnedOps + Send + 'static> Send for OwnershipState<T> {}

/// Represents a block device.
/// 
/// `Device` wraps an `spdk_bdev` pointer and can be in one of three ownership
/// states: owned, borrowed, or none.
/// 
/// An owned device owns the underlying `spdk_bdev` pointer and will destroy it
/// when dropped. The caller must ensure that the drop occurs in the same thread
/// that created the device. It must also occur as part of thread event handling
/// by explicitly calling [`task::yield_now`] before dropping the device.
/// However, it is easiest and safest to explicitly call [`Device<T>::destroy`]
/// on the device rather than let it drop naturally.
/// 
/// A borrowed device borrows the underlying `spdk_bdev` pointer. Dropping a
/// borrowed device has no effect on the underlying `spdk_bdev` pointer.
/// 
/// A device with no ownership state can only be safely queried for ownership
/// state or dropped. Any other operation will panic. A device will be left in
/// this state after the [`Device<T>::take`] method is called.
/// 
/// [`Device<T>::destroy`]: method@Device<T>::destroy
/// [`Device<T>::take`]: method@Device<T>::take
/// [`task::yield_now`]: function@crate::task::yield_now
pub struct Device<T: OwnedOps + Send + From<Owned> + 'static>(OwnershipState<T>);

unsafe impl <T: OwnedOps + Send + From<Owned> + 'static> Send for Device<T> {}
unsafe impl <T: OwnedOps + Send + From<Owned> + 'static> Sync for Device<T> {}

impl <T: OwnedOps + Send + From<Owned> + 'static> Device<T> {
    /// Get an owned [`Device`] for a block device.
    pub fn new(dev: T) -> Self {
        Self(OwnershipState::Owned(dev))
    }

    /// Get a borrowed [`Device`] by its name.
    /// 
    /// # Returns
    /// 
    /// This function returns [`None`] if no block device with the given name
    /// exists.
    pub fn from_name(name: &CStr) -> Option<Device<Any>> {
        let bdev = unsafe { spdk_bdev_get_by_name(name.as_ptr()) };

        match NonNull::new(bdev) {
            Some(b) => {
                Some(Device::<Any>(OwnershipState::Borrowed(b)))
            },
            None => None,
        }
    }

    /// Get a borrowed [`Device`] for a raw `spdk_bdev` pointer.
    pub fn from_ptr(bdev: *mut spdk_bdev) -> Device<Any> {
        match NonNull::new(bdev) {
            Some(b) => {
                Device::<Any>(OwnershipState::Borrowed(b))
            },
            None => panic!("device pointer must not be null"),
        }
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

    /// Consumes this device and returns a [`Device<Owned>`] assuming ownership
    /// of the underlying `spdk_bdev` pointer.
    pub fn into_owned(&mut self) -> Option<Device<Owned>> {
        match self.0 {
            OwnershipState::Owned(_) => {
                match mem::replace(&mut self.0, OwnershipState::None) {
                    OwnershipState::Owned(dev) => {
                        Some(Owned::new(dev))
                    },
                    _ => unreachable!(),
                }
            },
            _ => None,
        }
    }

    /// Borrow this device.
    pub fn borrow(&self) -> Device<Any> {
        match &self.0 {
            OwnershipState::Owned(dev) => {
                Device::<Any>(OwnershipState::Borrowed(
                    unsafe { NonNull::new_unchecked(dev.as_ptr()) }
                ))
            },
            OwnershipState::Borrowed(bdev) => {
                Device::<Any>(OwnershipState::Borrowed(bdev.clone()))
            },
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

    /// Takes the value from this device and replaces with a value having no
    /// ownership.
    pub fn take(&mut self) -> Self {
        mem::replace(self, Self(OwnershipState::None))
    }

    /// Destroy the block device asynchronously.
    /// 
    /// # Returns
    /// 
    /// Only an owned device can be destroyed. This function returns `Err(EPERM)`
    /// if called on a borrowed device and `Err(ENODEV)` if called on a device
    /// that neither owns nor borrows the underlying `spdk_bdev` pointer.
    pub async fn destroy(mut self) -> Result<(), Errno> {
        match self.0 {
            OwnershipState::Borrowed(_) => Err(EPERM),
            OwnershipState::None => Err(ENODEV),
            OwnershipState::Owned(_) => {
                match mem::replace(&mut self.0, OwnershipState::None) {
                    OwnershipState::Owned(dev) => {
                        dev.destroy().await
                    },
                    _ => unreachable!(),
                }
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

impl <T: OwnedOps + Send + From<Owned> + 'static> Drop for Device<T> {
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

            Some(Device::<Any>::from_ptr(current))
        }
    }
}

/// Get an iterator over all block devices.
pub fn devices() -> Devices {
    Devices(unsafe { spdk_bdev_first() })
}
