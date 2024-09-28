//! Asynchronous networking primitives for TCP communication using the Storage Performance
//! Development Toolkit.
//!
//! This module provides asynchronous networking functionality for the Transmission Control Protocol
//! using the Storage Performance Development Kit socket library and modules. It also provides
//! services for converting and resolving socket addresses.
mod addr;
mod listener;
mod socket;
mod stream;

pub use addr::{resolve, SocketAddr, SocketAddrIter, ToSocketAddrs};
pub use listener::{Accepted, Incoming, TcpListener};
pub(crate) use socket::AsRawSock;
pub use socket::{TcpSocketExt, TcpSocketRemote};
#[allow(unused_imports)]
use spdk_sys::spdk_sock;
pub use stream::TcpStream;
