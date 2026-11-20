//! # Overview
//!
//! A crate for writing scalable, high performance, asynchronous user-mode storage applications in
//! Rust using the Storage Performance Development Kit ([SPDK]).
//!
//! The `rust-spdk` crate provides low- to zero-cost abstractions in Rust built on features and
//! concepts of the SPDK. It implements an asynchronous runtime using the SPDK's [Message Passing
//! and Concurrency][SPMP] and [Event Framework][SPEF].
//!
//! Creating an asynchronous `rust-spdk`-based application is as simple as adding the [`spdk::main`]
//! attribute to your `async fn main()` function. The `main` function is run on a special, dedicated
//! SPDK application thread. This thread is always associated with the main CPU core assigned to the
//! application.
//!
//! # Example
//!
//! The following example creates a basic `rust-spdk` application that creates an instance of the
//! [`Malloc`] RAM disk block device and then writes to and reads from it.
//!
//! ```no_run
#![doc = include_str!("../examples/bdev_hello_world.rs")]
//! ```
//!
//! Additional examples can be found [here][RSEX].
//!
//! # Feature Flags
//!
#![doc = document_features::document_features!()]
//!
//! [`Malloc`]: crate::bdev::Malloc
//! [`spdk::main`]: crate::main
//! [RSEX]: https://github.com/edreed/rust-spdk/tree/main/spdk/examples
//! [SPDK]: https://www.spdk.io/doc/about.html
//! [SPEF]: https://spdk.io/doc/event.html
//! [SPMP]: https://spdk.io/doc/concurrency.html
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod alloc;
pub mod bdev;
pub mod cli;
pub mod dma;
pub mod env;
pub mod errors;
pub mod json;
pub mod net;
pub mod nvme;
pub mod nvmf;
pub mod runtime;
pub mod scsi;
pub mod sync;
pub mod task;
pub mod thread;
pub mod time;

pub(crate) mod macros;

mod uuid;

pub use spdk_macros::main;

#[cfg(feature = "bdev-module")]
pub use spdk_macros::module;

#[cfg(feature = "json-rpc")]
pub use spdk_macros::jsonrpc;

pub use uuid::Uuid;

/// A specialized `Result` type for SPDK operations.
pub type Result<T> = std::result::Result<T, errors::Errno>;
