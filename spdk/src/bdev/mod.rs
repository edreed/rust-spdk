//! Support for the Storage Performance Development Kit plug-in block device modules.
//! 
//! See [Block Device User Guide] for more information about available plug-ins.
//! 
//! [Block Device User Guide]: https://spdk.io/doc/bdev.html
mod any;
mod bdev;

#[cfg(feature = "bdev-malloc")]
pub mod malloc;

pub use any::Any;
pub use bdev::BDev;
