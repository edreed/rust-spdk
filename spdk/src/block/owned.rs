use std::{
    pin::Pin,
    ptr::NonNull,
};

use async_trait::async_trait;
use futures::Future;
use spdk_sys::spdk_bdev;

use crate::{
    block::Device,
    errors::Errno,
};

/// A trait for owned block devices.
#[async_trait]
pub trait OwnedOps: From<Owned> + Send + 'static {
    /// Returns a pointer to the underlying `spdk_bdev` structure.
    fn as_ptr(&self) -> *mut spdk_bdev;

    /// Destroy the block device asynchronously.
    async fn destroy(self) -> Result<(), Errno>;
}

type DestroyFn = fn(Owned) -> Pin<Box<(dyn Future<Output = Result<(), Errno>> + Send)>>;

/// Destroys the type-erased device managed by the specified [`Owned`] instance.
fn destroy_device<T>(owned: Owned) -> Pin<Box<(dyn Future<Output = Result<(), Errno>> + Send)>>
where
    T: OwnedOps
{
    let device: T = owned.into();

    Box::pin(async move {
        device.destroy().await
    })
}

/// Represents a type-erased owned block device.
pub struct Owned {
    bdev: NonNull<spdk_bdev>,
    destroy_fn: DestroyFn,
}

unsafe impl Send for Owned {}

impl Owned {
    /// Consumes the specified device and returns a new [`Device<Owned>`]
    /// instance.
    pub(crate) fn new<T>(device: T) -> Device<Self>
    where
        T: OwnedOps
    {
        Device::new(Self {
            bdev: unsafe { NonNull::new_unchecked(device.as_ptr()) },
            destroy_fn: destroy_device::<T>,
        })
    }

    /// Consumes this device and returns a pointer to the underlying `spdk_bdev`
    /// structure.
    pub fn into_ptr(self) -> *mut spdk_bdev {
        self.bdev.as_ptr()
    }
}

#[async_trait]
impl OwnedOps for Owned {
    fn as_ptr(&self) ->  *mut spdk_bdev {
        self.bdev.as_ptr()
    }

    async fn destroy(self) -> Result<(), Errno> {
        (self.destroy_fn)(self).await
    }
}
