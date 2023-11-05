use async_trait::async_trait;
use spdk_sys::Errno;

/// A trait for block devices.
#[async_trait]
pub trait BDev: Sized {
    /// Destroy the block device asynchronously.
    async fn destroy(self) -> Result<(), Errno>;
}
