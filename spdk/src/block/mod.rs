//! Support for Storage Performance Development Kit block devices.
#![cfg(feature = "bdev")]
mod any;
mod descriptor;
mod device;
mod io_channel;
mod owned;

pub use any::Any;
pub use descriptor::Descriptor;
pub use device::{
    Device,
    Devices,
    IoType,

    devices
};
pub use io_channel::IoChannel;
pub use owned::{
    Owned,
    OwnedOps,
};
