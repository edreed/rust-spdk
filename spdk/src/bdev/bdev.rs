use async_trait::async_trait;
use spdk_sys::spdk_bdev;

use crate::errors::Errno;

/// A trait for block devices.
#[async_trait]
pub trait BDev: Send {
    /// Returns a pointer to the underlying `spdk_bdev` structure.
    fn as_ptr(&self) -> *mut spdk_bdev;

    /// Destroy the block device asynchronously.
    async fn destroy(self) -> Result<(), Errno>;
}
