//! Support for the Storage Performance Development Kit plug-in block device modules.
//! 
//! See [Block Device User Guide] for more information about available plug-ins.
//! 
//! [Block Device User Guide]: https://spdk.io/doc/bdev.html
#[cfg(feature = "bdev-malloc")]
pub mod malloc;