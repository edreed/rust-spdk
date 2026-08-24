//! Support for Storage Performance Development Kit block devices.
#![cfg(feature = "bdev")]
mod any;
mod descriptor;
mod device;
mod dif;
mod io;
mod io_channel;
mod owned;

pub use any::Any;
pub use descriptor::Descriptor;
pub use device::{Device, Devices, devices};
pub use dif::{
    CheckFlag as DifCheckFlag, CheckType as DifCheckType, PiFormat as DifPiFormat, Type as DifType,
};
pub use io::{IoError, IoResult, IoType};
pub use io_channel::IoChannel;
pub use owned::{Owned, OwnedOps};
