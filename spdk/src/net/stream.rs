use std::{ffi::CString, ptr::{self, addr_of_mut, NonNull}};

use spdk_sys::{spdk_sock, spdk_sock_connect, spdk_sock_getaddr};

use crate::errors::{self, Errno, EINVAL, ENOTCONN};

use super::{AsRawSock, SocketAddr, ToSocketAddrs};

pub struct TcpStream(NonNull<spdk_sock>);

impl TcpStream {
    pub(crate) fn from_ptr(sock: *mut spdk_sock) -> Option<Self> {
        NonNull::new(sock).map(Self)
    }

    pub async fn connect<A: ToSocketAddrs>(addrs: A) -> Result<Self, errors::Errno> {
        for addr in addrs.to_socket_addr().await? {
            let stream = unsafe {
                spdk_sock_connect(
                    addr.ip().as_ptr(),
                    addr.port() as i32,
                    ptr::null())
            };

            match NonNull::new(stream) {
                Some(sock) => return Ok(Self(sock)),
                None => continue,
            }
        }

        Err(EINVAL)
    }

    pub fn peer_addr(&self) -> Result<SocketAddr, Errno> {
        let mut addr = [0u8; 46];
        let mut port = 0u16;

        if unsafe {
            spdk_sock_getaddr(
                self.0.as_ptr(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                addr.as_mut_ptr().cast(),
                addr.len() as i32,
                addr_of_mut!(port),
            ) == 0 }
        {
            return Ok(SocketAddr::new(CString::from_vec_with_nul(addr.into()).unwrap(), port));
        }

        Err(ENOTCONN)
    }
}

impl AsRawSock for TcpStream {
    fn as_raw_sock(&self) -> super::RawSock {
        self.0.as_ptr()
    }
}