//! Asynchronous networking primitives for TCP communication using the Storage Performance
//! Development Toolkit.
//!
//! This module provides asynchronous networking functionality for the Transmission Control Protocol
//! using the Storage Performance Development Kit socket library and modules. It also provides
//! services for converting and resolving socket addresses.
mod addr;
pub mod libanl;
mod listener;
mod sock_group;
mod stream;

pub use addr::{resolve, SocketAddr, SocketAddrIter, ToSocketAddrs};
pub use listener::TcpListener;
#[allow(unused_imports)]
pub(crate) use sock_group::{SocketEvent, SocketGroup};
use spdk_sys::spdk_sock;
pub use stream::TcpStream;

pub(crate) trait AsRawSock {
    fn as_raw_sock(&self) -> *mut spdk_sock;
}
