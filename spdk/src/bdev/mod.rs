//! Support for the Storage Performance Development Kit plug-in block device modules.
//!
//! See [Block Device User Guide] for more information about available plug-ins.
//!
//! See [Writing a Custom Block Device Module] for more information about
//! writing a custom plug-in block device module.
//!
//! [Block Device User Guide]: https://spdk.io/doc/bdev.html
//! [Writing a Custom Block Device Module]: https://spdk.io/doc/bdev_module.html
#[cfg(feature = "bdev-malloc")]
pub mod malloc;

#[cfg(feature = "bdev-module")]
#[allow(clippy::module_inception)]
mod bdev;
#[cfg(feature = "bdev-module")]
mod module;

#[cfg(feature = "bdev-module")]
pub(crate) use bdev::BDevIoCtx;
#[cfg(feature = "bdev-module")]
pub use bdev::{BDevImpl, BDevIo, BDevIoChannel, BDevIoChannelOps, BDevOps, IoStatus};

#[cfg(feature = "bdev-module")]
pub use module::{Module, ModuleInstance, ModuleOps};
