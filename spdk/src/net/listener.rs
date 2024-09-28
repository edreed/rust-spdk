use std::ptr::NonNull;

use spdk_sys::spdk_sock;

use super::addr::ToSocketAddrs;

pub struct TcpListener(NonNull<spdk_sock>);

impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Self {
        unimplemented!()
    }
}