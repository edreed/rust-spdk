//! Support for the Storage Performance Development Kit Malloc Block Device
//! plug-in.
use std::{
    default::Default,
    ffi::CStr,
    mem::{
        self,

        ManuallyDrop,
    },
    os::raw:: c_char,
    ptr::{
        NonNull,

        null_mut,
    },
    task::Poll,
};

use async_trait::async_trait;
use spdk_sys::{
    malloc_bdev_opts,
    spdk_bdev,

    create_malloc_disk,
    delete_malloc_disk, 
    spdk_bdev_get_name,
};

use crate::{
    block::Device,
    errors::Errno,
    task::{
        Promise,

        complete_with_status,
    },
    to_result,
};

use super::BDev;

/// Builds a [`Malloc`] instance using the Malloc Block Device module of the
/// SPDK.
/// 
/// `Builder` implements a fluent-style interface enabling custom configuration
/// through chaining function calls. The [`build`] method constructs a new
/// `Malloc` instance.
/// 
/// [`build`]: Builder::build
pub  struct Builder(malloc_bdev_opts);

unsafe impl Send for Builder {}

impl Builder {
    /// Returns a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the device name.
    pub fn with_name(mut self, name: &CStr) -> Self {
        self.0.name = name.as_ptr() as *mut c_char;
        self
    }

    /// Sets the total capacity of the device in blocks.
    pub fn with_num_blocks(mut self, num_blocks: u64) -> Self {
        self.0.num_blocks = num_blocks;
        self
    }

    /// Sets the block size, in bytes.
    pub fn with_block_size(mut self, block_size: u32) -> Self {
        self.0.block_size = block_size;
        self
    }

    /// Creates a new [`Device<Malloc>`] instance that owns the underlying
    /// `spdk_bdev` pointer.
    /// 
    /// # Notes
    /// 
    /// The returned [`Device<Malloc>`] instance owns the underlying `spdk_bdev`
    /// pointer and will destroy it when dropped. See [`Device<T>`] for a detailed
    /// discussion of ownership semantics and requirements.
    /// 
    /// [`Device<Malloc>::destroy`]: method@crate::block::Device<Malloc>::destroy
    /// [`task::yield_now`]: function@crate::task::yield_now
    pub fn build(self) -> Result<Device<Malloc>, Errno>{
        let mut malloc = null_mut();
        
        unsafe { to_result!(create_malloc_disk(&mut malloc, &self.0))? };

        Ok(Device::from_ptr_owned(malloc))
    }
}

impl Default for Builder {
    fn default() -> Self {
        unsafe { mem::zeroed::<Self>() }
    }
}

/// Represents a Malloc Block Device.
pub struct Malloc(NonNull<spdk_bdev>);

unsafe impl Send for Malloc {}

impl Malloc {
    /// Returns a pointer to the underlying `spdk_bdev` structure.
    fn as_ptr(&self) -> *mut spdk_bdev {
        self.0.as_ptr()
    }
}

#[async_trait]
impl BDev for Malloc {
    async fn destroy(self) -> Result<(), Errno> {
        Promise::new(move |cx| {
            unsafe {
                delete_malloc_disk(
                    spdk_bdev_get_name(self.as_ptr()),
                    Some(complete_with_status),
                    cx);
            }

            Poll::Pending
        }).await
    }
}

impl From<Device<Malloc>> for Malloc {
    fn from(device: Device<Malloc>) -> Self {
        Self(NonNull::new(ManuallyDrop::new(device).as_ptr()).unwrap())
    }
}
