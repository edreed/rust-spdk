mod addr;
pub mod libanl;
mod listener;
mod stream;
mod sock_group;

pub use addr::{
    SocketAddrIter,
    SocketAddr,
    ToSocketAddrs,

    lookup_host,
};
pub use listener::TcpListener;
use spdk_sys::spdk_sock;
pub use stream::TcpStream;

type RawSock = *mut spdk_sock;

pub(crate) trait AsRawSock {
    fn as_raw_sock(&self) -> RawSock;
}