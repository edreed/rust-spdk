//! Support for Storage Performance Development Kit block devices.
mod descriptor;
mod device;
mod io_channel;
mod io;

pub use descriptor::Descriptor;

pub use device::Device;
pub use device::Devices;
pub use device::devices;

pub use io_channel::IoChannel;

pub(crate) use io::Io;
