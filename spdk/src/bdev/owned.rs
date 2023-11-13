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

use super::BDev;

type DestroyFn = fn(Owned) -> Pin<Box<(dyn Future<Output = Result<(), Errno>> + Send)>>;

/// Destroys the type-erased device managed by the specified [`Owned`] instance.
fn destroy_device<T>(owned: Owned) -> Pin<Box<(dyn Future<Output = Result<(), Errno>> + Send)>>
where
    T: BDev + From<Owned> + Send + 'static
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
        T: BDev + From<Owned> + Send + 'static
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
impl BDev for Owned {
    fn as_ptr(&self) ->  *mut spdk_bdev {
        self.bdev.as_ptr()
    }

    async fn destroy(self) -> Result<(), Errno> {
        (self.destroy_fn)(self).await
    }
}
