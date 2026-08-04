use std::{future::Future, mem, pin::Pin, ptr::NonNull};

use spdk_sys::spdk_bdev;

use crate::{Result, block::Device};

/// A trait for owned block devices.
pub trait OwnedOps: From<Owned> + 'static {
    /// Returns a pointer to the underlying `spdk_bdev` structure.
    fn as_ptr(&self) -> *mut spdk_bdev;

    /// Destroy the block device asynchronously.
    fn destroy(self) -> impl Future<Output = Result<()>>;
}

type DestroyFn = fn(Owned) -> Pin<Box<dyn Future<Output = Result<()>>>>;

/// Destroys the type-erased device managed by the specified [`Owned`] instance.
fn destroy_device<T>(owned: Owned) -> Pin<Box<dyn Future<Output = Result<()>>>>
where
    T: OwnedOps,
{
    let device: T = owned.into();

    Box::pin(async move { device.destroy().await })
}

/// Represents a type-erased owned block device.
pub struct Owned {
    bdev: NonNull<spdk_bdev>,
    destroy_fn: DestroyFn,
}

impl Owned {
    /// Consumes the specified device and returns a new [`Device<Owned>`]
    /// instance.
    pub(crate) fn new<T>(device: T) -> Device<Self>
    where
        T: OwnedOps,
    {
        // Since `Device` is taking ownership of the BDev via its `spdk_bdev`
        // pointer, we need to ensure that the BDev is not dropped here if the
        // value is a smart pointer type.
        let device = mem::ManuallyDrop::new(device);

        Device::new(Self {
            // SAFETY: The pointer is guaranteed to be non-null by the wrapper
            // passed to this function.
            bdev: unsafe { NonNull::new_unchecked(device.as_ptr()) },
            destroy_fn: destroy_device::<T>,
        })
    }

    /// Consumes this device and returns a pointer to the underlying `spdk_bdev`
    /// structure.
    ///
    /// After calling this function, the caller is responsible for managing the
    /// memory previously owned by this device.
    pub fn into_ptr(self) -> *mut spdk_bdev {
        mem::ManuallyDrop::new(self).bdev.as_ptr()
    }
}

impl OwnedOps for Owned {
    fn as_ptr(&self) -> *mut spdk_bdev {
        self.bdev.as_ptr()
    }

    async fn destroy(self) -> Result<()> {
        (self.destroy_fn)(self).await
    }
}
