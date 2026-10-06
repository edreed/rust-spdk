use std::{
    future::Future,
    marker::PhantomData,
    mem::{self, transmute},
    pin::Pin,
    ptr::NonNull,
};

use spdk_sys::spdk_bdev;

use crate::{
    Result,
    block::{AsRawBDev, Device},
};

/// A trait for owned block devices.
pub trait OwnedOps: AsRawBDev + From<Owned> {
    /// Destroy the block device asynchronously.
    fn destroy(self) -> impl Future<Output = Result<()>>;
}

type DestroyFn = fn(Owned) -> Pin<Box<dyn Future<Output = Result<()>>>>;

/// Destroys the type-erased device managed by the specified [`Owned`] instance.
fn destroy_device<T>(owned: Owned) -> Pin<Box<dyn Future<Output = Result<()>>>>
where
    T: OwnedOps,
{
    Box::pin(async move {
        let device: T = owned.into();

        device.destroy().await
    })
}

/// Represents a type-erased owned block device.
pub struct Owned {
    bdev: NonNull<spdk_bdev>,
    destroy_fn: DestroyFn,
}

unsafe impl Sync for Owned {}

impl Owned {
    /// Consumes the specified device and returns a new [`Device<Owned>`] instance.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the provided device is valid and that the resulting
    /// `Device<Owned>` will be the only instance owning the underlying `spdk_bdev` pointer.
    pub(crate) unsafe fn new<T>(device: T) -> Device<Self>
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
            bdev: unsafe { NonNull::new_unchecked(device.as_raw_bdev()) },
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

impl AsRawBDev for Owned {
    fn as_raw_bdev(&self) -> *mut spdk_bdev {
        self.bdev.as_ptr()
    }
}

impl OwnedOps for Owned {
    async fn destroy(self) -> Result<()> {
        (self.destroy_fn)(self).await
    }
}

/// Represents a block device that has a lifetime owned by a specific type.
///
/// This type is used to scope the lifetime of a borrowed block device to the lifetime of its owner,
/// `T`.
#[repr(transparent)]
pub struct OwnedBy<'a, T>(NonNull<spdk_bdev>, PhantomData<&'a T>);

unsafe impl<'a, T> Send for OwnedBy<'a, T> {}
unsafe impl<'a, T> Sync for OwnedBy<'a, T> {}

impl<'a, T> OwnedBy<'a, T> {
    /// Creates a new `OwnedBy` instance from a raw `spdk_bdev` pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the provided `bdev` pointer is valid and non-null.
    pub(crate) unsafe fn new_unchecked(bdev: *mut spdk_bdev) -> Self {
        // SAFETY: This type is a transparent wrapper around `NonNull<spdk_bdev>`.
        unsafe { transmute(NonNull::new_unchecked(bdev)) }
    }
}

impl<T> AsRawBDev for OwnedBy<'_, T> {
    fn as_raw_bdev(&self) -> *mut spdk_bdev {
        self.0.as_ptr()
    }
}
