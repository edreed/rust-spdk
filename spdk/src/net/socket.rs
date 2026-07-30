//! Common operations that can be performed on TCP listeners and streams.
use std::{
    ffi::CStr,
    mem::MaybeUninit,
    ptr::{self, addr_of_mut},
};

use spdk_sys::{spdk_sock, spdk_sock_getaddr};

use crate::{errors::Errno, to_result};

use super::SocketAddr;

/// A trait providing access to the raw `spdk_sock` pointer.
pub(crate) trait AsRawSock {
    /// Returns the socket's raw `spdk_sock` pointer.
    fn as_raw_sock(&self) -> *mut spdk_sock;
}

impl<T> AsRawSock for MaybeUninit<T>
where
    T: AsRawSock,
{
    fn as_raw_sock(&self) -> *mut spdk_sock {
        unreachable!("as_raw_sock called on uninitialized data");
    }
}

/// A trait defining methods common to TCP sockets.
#[allow(private_bounds)]
pub trait TcpSocketExt: AsRawSock {
    /// Returns the local address to which this TCP socket is bound.
    fn local_addr(&self) -> Result<SocketAddr, Errno> {
        let mut addr = [0u8; 46];
        let mut port = 0u16;

        let res = to_result!(unsafe {
            spdk_sock_getaddr(
                self.as_raw_sock(),
                addr.as_mut_ptr().cast(),
                addr.len() as i32,
                addr_of_mut!(port),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
            )
        });

        res.map(|_| SocketAddr::new(CStr::from_bytes_until_nul(&addr).unwrap().into(), port))
    }
}

/// A trait defining methods common to TCP sockets with remote connections.
pub trait TcpSocketRemote: TcpSocketExt {
    /// Returns the socket address of the remote connection.
    fn peer_addr(&self) -> Result<SocketAddr, Errno> {
        let mut addr = [0u8; 46];
        let mut port = 0u16;

        let res = to_result!(unsafe {
            spdk_sock_getaddr(
                self.as_raw_sock(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                addr.as_mut_ptr().cast(),
                addr.len() as i32,
                addr_of_mut!(port),
            )
        });

        res.map(|_| SocketAddr::new(CStr::from_bytes_until_nul(&addr).unwrap().into(), port))
    }
}
