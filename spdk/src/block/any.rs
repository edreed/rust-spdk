use spdk_sys::spdk_bdev;

use crate::errors::{EPERM, Errno};

use super::{Owned, OwnedOps};

/// A placeholder type that represents any block device.
pub struct Any;

impl OwnedOps for Any {
    fn as_ptr(&self) -> *mut spdk_bdev {
        unreachable!("Any::as_ptr() should never be called")
    }

    async fn destroy(self) -> Result<(), Errno> {
        Err(EPERM)
    }
}

impl From<Owned> for Any {
    fn from(_owned: Owned) -> Self {
        unreachable!("Any::from_owned() should never be called")
    }
}
