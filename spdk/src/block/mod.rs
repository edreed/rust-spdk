//! Support for Storage Performance Development Kit block devices.
#![cfg(feature = "bdev")]
mod any;
#[cfg(feature = "bdev-module")]
mod claim;
mod descriptor;
mod device;
mod dif;
mod io;
mod io_channel;
mod owned;

pub use any::Any;
pub use descriptor::Descriptor;
pub use device::{AsRawBDev, Device, devices};
pub use dif::{
    CheckFlag as DifCheckFlag, CheckType as DifCheckType, PiFormat as DifPiFormat, Type as DifType,
};
pub use io::{IoError, IoResult, IoType};
pub use io_channel::IoChannel;
pub use owned::{Owned, OwnedBy, OwnedOps};

#[cfg(feature = "bdev-module")]
pub use claim::ClaimType;
use spdk_sys::spdk_bdev_wait_for_examine;

use crate::task::{Promise, Promissory};

use super::{Result, to_poll_pending_on_ok};

/// Waits for the examination process to finish on all block devices.
pub async fn wait_for_examination() -> Result<()> {
    Promise::new()
        .request(|p| {
            let (cb_fn, cb_arg) = Promissory::callback_with_ok(p);

            to_poll_pending_on_ok! {
                unsafe { spdk_bdev_wait_for_examine(Some(cb_fn), cb_arg as *const _ as *mut _) }
                => on ready {
                    unsafe { drop(Promissory::from_raw(cb_arg)) };
                }
            }
            .map_err(Into::into)
        })
        .await
}
