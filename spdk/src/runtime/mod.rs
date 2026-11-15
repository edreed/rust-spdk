//! Asynchronous runtime support for the Storage Performance Development Kit
//! [Event Framework][SPEF].
//!
//! [SPEF]: https://spdk.io/doc/event.html
mod reactor;
#[allow(clippy::module_inception)]
mod runtime;

pub use reactor::{Reactor, reactors, spawn_local, spawn_local_detached};
pub use runtime::{Builder, Runtime, is_shutting_down, shutdown, wait_for_shutdown};
