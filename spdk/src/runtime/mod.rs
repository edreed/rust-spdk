//! Asynchronous runtime support for the Storage Performance Development Kit
//! [Event Framework][SPEF].
//!
//! [SPEF]: https://spdk.io/doc/event.html
mod cpu_core;
mod cpu_set;
mod reactor;
#[allow(clippy::module_inception)]
mod runtime;

pub use cpu_core::{CpuCore, CpuCores, cpu_cores};

pub use cpu_set::CpuSet;

pub use reactor::{Reactor, reactors, spawn_local, spawn_local_detached};

pub use runtime::{Builder, Runtime};
