//! Support for Storage Performance Development Kit [Event Framework][SPEF].
//! 
//! [SPEF]: https://spdk.io/doc/event.html
mod cpu_core;
mod runtime;

pub use cpu_core::CpuCore;
pub use cpu_core::CpuCores;
pub use cpu_core::cpu_cores;

pub use runtime::Builder;
pub use runtime::Runtime;