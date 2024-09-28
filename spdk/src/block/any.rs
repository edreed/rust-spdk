use std::{
    future::{
        Future,

        ready,
    },
    pin::Pin,
};

use spdk_sys::spdk_bdev;

use crate::errors::{
    Errno,

    EPERM,
};

use super::{
    OwnedOps,
    Owned,
};

/// A placeholder type that represents any block device.
pub struct Any;

unsafe impl Send for Any {}

impl OwnedOps for Any {
    fn as_ptr(&self) -> *mut spdk_bdev {
        unreachable!("Any::as_ptr() should never be called")
    }
    
    fn destroy(self) -> Pin<Box<(dyn Future<Output = Result<(), Errno>> + Send)>> {
        Box::pin(ready(Err(EPERM)))
    }
}

impl From<Owned> for Any {
    fn from(_owned: Owned) -> Self {
        unreachable!("Any::from_owned() should never be called")
    }
}
