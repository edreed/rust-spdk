use std::mem;

use async_trait::async_trait;
use spdk_sys::Errno;

use crate::{
    block::Device,
    errors::EPERM,
};

use super::BDev;

/// A placeholder type that represents any block device.
pub struct Any;

#[async_trait]
impl BDev for Any {
    async fn destroy(self) -> Result<(), Errno> {
        Err(EPERM)
    }
}

impl From<Device<Any>> for Any {
    fn from(dev: Device<Any>) -> Self {
        // We should never be able to get an owned device here. If we do, we
        // would leak the block device.
        assert!(!dev.is_owned());

        mem::forget(dev);
        Self
    }
}
