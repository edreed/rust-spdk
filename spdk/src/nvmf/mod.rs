//! Support for the SPDK NVMe over Fabrics (NVMe-oF) target.
#![cfg(feature = "nvmf")]
mod host;
mod namespace;
mod subsystem;
mod target;
mod transport;

pub use host::{AllowedHosts, Host};
pub use namespace::{Namespace, Namespaces};
pub use subsystem::{Subsystem, SubsystemType, Subsystems};
pub use target::{Builder as TargetBuilder, Target, Targets, targets};
pub use transport::{Builder as TransportBuilder, Transport, TransportType};
