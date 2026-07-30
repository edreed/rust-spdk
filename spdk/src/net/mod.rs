//! Asynchronous networking primitives for TCP communication using the Storage Performance
//! Development Toolkit.
//!
//! This module provides asynchronous networking functionality for the Transmission Control Protocol
//! using the Storage Performance Development Kit socket library and modules. It also provides
//! services for converting and resolving socket addresses.
mod addr;
mod group;
mod listener;
mod polled;
mod socket;
mod stream;

pub(crate) use group::SocketGroupEvent;
pub(crate) use listener::{RawTcpListener, RawTcpListenerVtable, TcpListenerSocket};
pub(crate) use polled::{accept_polled_stream, bind_polled_listener, connect_polled_stream};
pub(crate) use socket::AsRawSock;
pub(crate) use stream::{Connector, RawTcpStream, RawTcpStreamVtable, TcpStreamSocket};

pub use addr::{SocketAddr, SocketAddrIter, ToSocketAddrs, resolve};
pub use group::SocketGroup;
pub use listener::{Accepted, Incoming, TcpListener};
pub use socket::{TcpSocketExt, TcpSocketRemote};
#[allow(unused_imports)]
use spdk_sys::spdk_sock;
pub use stream::TcpStream;
