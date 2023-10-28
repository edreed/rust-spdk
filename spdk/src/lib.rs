//! Rust programming language abstractions for the Storage Performance
//! Development Kit ([SPDK]).
//! 
//! [SPDK]: https://www.spdk.io

pub mod bdev;
pub mod block;
pub mod dma;
pub mod errors;
pub mod nvme;
pub mod nvmf;
pub mod runtime;
pub mod task;
pub mod thread;
pub mod time;

pub use spdk_macros::main;
