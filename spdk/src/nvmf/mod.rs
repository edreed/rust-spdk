//! Support for the SPDK NVMe over Fabrics (NVMe-oF) target.
#![cfg(feature = "nvmf")]
mod host;
mod namespace;
mod subsystem;
mod target;
mod transport;

pub use host::{
    Host,
    AllowedHosts,
};
pub use namespace::{
    Namespace,
    Namespaces,
};
pub use subsystem::{
    Subsystem,
    Subsystems,
    SubsystemType,
};
pub use target::{
    Target,
    Targets,

    targets,
};
pub use transport::{
    Builder as TransportBuilder,
    Transport,
    TransportType,
};

use std::ffi::CStr;

pub const SPDK_NVMF_DISCOVERY_NQN: &CStr = unsafe {
    CStr::from_bytes_with_nul_unchecked(spdk_sys::SPDK_NVMF_DISCOVERY_NQN)
};
pub const SPDK_NVMF_NQN_UUID_PRE: &CStr = unsafe {
    CStr::from_bytes_with_nul_unchecked(spdk_sys::SPDK_NVMF_NQN_UUID_PRE)
};
