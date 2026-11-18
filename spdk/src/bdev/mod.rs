//! Support for the Storage Performance Development Kit plug-in block device modules.
//!
//! See [Block Device User Guide] for more information about available plug-ins.
//!
//! See [Writing a Custom Block Device Module] for more information about
//! writing a custom plug-in block device module.
//!
//! [Block Device User Guide]: https://spdk.io/doc/bdev.html
//! [Writing a Custom Block Device Module]: https://spdk.io/doc/bdev_module.html
#[cfg(feature = "bdev-aio")]
#[path = "."]
mod bdev_aio {
    mod aio;

    pub use aio::Aio;
}

#[cfg(feature = "bdev-aio")]
pub use bdev_aio::*;

#[cfg(feature = "bdev-malloc")]
#[path = "."]
mod bdev_malloc {
    mod malloc;

    pub use malloc::{Builder as MallocBuilder, Malloc};
}

#[cfg(feature = "bdev-malloc")]
pub use bdev_malloc::*;

#[cfg(feature = "bdev-module")]
#[path = "."]
mod bdev_module {
    mod bdev_impl;
    mod io_channel_impl;
    mod io_request;
    mod module;

    pub use bdev_impl::{BDevImpl, BDevOps, Builder as BDevImplBuilder};
    pub use io_channel_impl::IoChannelImpl;
    pub use io_request::IoRequest;
    pub use module::{Examiner, Module, ModuleInstance, ModuleOps};
}

#[cfg(feature = "bdev-module")]
pub use bdev_module::*;

#[cfg(feature = "bdev-uring")]
#[path = "."]
mod bdev_uring {
    mod uring;

    pub use uring::{Builder as UringBuilder, Uring};
}

#[cfg(feature = "bdev-uring")]
pub use bdev_uring::*;
