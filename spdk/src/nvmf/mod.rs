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
    Builder as TargetBuilder,
    Target,
    Targets,

    targets,
};
pub use transport::{
    Builder as TransportBuilder,
    Transport,
    TransportType,
};
