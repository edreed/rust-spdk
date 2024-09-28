use std::{ffi::{CStr, CString}, future::Future, pin::Pin, str::FromStr, task::{Context, Poll}};

use errno::Errno;

use crate::errors::EINVAL;

pub struct SocketAddr{
    host: CString,
    port: u16,
}

impl SocketAddr {
    pub fn new(host: CString, port: u16) -> Self {
        Self {
            host,
            port,
        }
    }

    pub fn host(&self) -> &CStr {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl FromStr for SocketAddr {
    type Err = Errno;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((host_str, port_str)) = s.rsplit_once(':') {
            let host = CString::new(host_str).map_err(|_| EINVAL)?;
            let port = str::parse(port_str).map_err(|_| EINVAL)?;

            return Ok(Self::new(host, port));
        }

        Err(EINVAL)
    }
}

pub struct ToSocketAddrsFuture;

impl Future for ToSocketAddrsFuture {
    type Output = Result<SocketAddr, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unimplemented!()
    }
}

pub trait ToSocketAddrs {
    type Iter: Iterator<Item=SocketAddr>;

    fn to_socket_addr(&self) -> ToSocketAddrsFuture;
}

