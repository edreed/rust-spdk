#![cfg(feature = "nvme")]

mod status;
mod transport;

pub use status::NvmeStatus;
pub use transport::{FibreChannelAddr, PciAddr, TransportAddr, TransportId};

/// A value used to indicate a command applies to all namespaces or to retrieve
/// global log pages.
pub const SPDK_NVME_GLOBAL_NS_TAG: u32 = 0xffffffff;
