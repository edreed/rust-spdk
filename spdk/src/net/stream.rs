use std::{
    ffi::CStr,
    future::Future,
    io::IoSlice,
    mem::{self, MaybeUninit},
    os::raw::c_void,
    pin::Pin,
    ptr::{self, addr_of, addr_of_mut},
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use spdk_sys::{
    iovec as IoVec, spdk_sock, spdk_sock_close, spdk_sock_connect_async,
    spdk_sock_get_default_opts, spdk_sock_getaddr, spdk_sock_is_connected, spdk_sock_opts,
    spdk_sock_recv, spdk_sock_writev,
};

use crate::{
    errors::{self, Errno, EAGAIN, EBADF, EINVAL, ENOTCONN},
    net::stream::StreamState::{Connecting, Failed},
    task::{Polled, Poller},
    to_result, to_result_size,
};

use super::{AsRawSock, SocketAddr, ToSocketAddrs};

#[derive(Debug)]
enum StreamState {
    Connecting(Option<Waker>),
    Connected {
        reader: Option<Waker>,
        writer: Option<Waker>,
    },
    Failed(errors::Errno),
}

impl StreamState {
    fn poll(&self) {
        match self {
            Self::Connecting(Some(waker)) => waker.wake_by_ref(),
            Self::Connected {
                reader: maybe_reader,
                writer: maybe_writer,
            } => {
                if let Some(reader) = maybe_reader {
                    reader.wake_by_ref();
                }
                if let Some(writer) = maybe_writer {
                    writer.wake_by_ref();
                }
            }
            _ => unreachable!("poll called in unexpected state: {:?}", self),
        }
    }
}

struct Connector(Option<TcpStream>);

impl Future for Connector {
    type Output = Result<TcpStream, errors::Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0
            .as_mut()
            .map(|s| s.poll_connected(cx))
            .unwrap_or(Poll::Ready(Err(EBADF)))
            .map_ok(|_| self.0.take().unwrap())
    }
}

struct TcpStreamInner {
    sock: *mut spdk_sock,
    state: StreamState,
}

impl Drop for TcpStreamInner {
    fn drop(&mut self) {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.sock as *mut _) }
        };

        res.map_err(|e| if e == EBADF { Ok(()) } else { Err(e) })
            .expect("socket closed");
    }
}

impl Polled for TcpStreamInner {
    fn poll(self: Pin<&mut Self>) -> bool {
        self.state.poll();
        true
    }
}

pub struct TcpStream(Poller<TcpStreamInner>);

impl TcpStream {
    pub(crate) fn from_ptr(sock: *mut spdk_sock) -> Option<Self> {
        if !sock.is_null() {
            return Some(Self(Poller::new(TcpStreamInner {
                sock,
                state: StreamState::Connected {
                    reader: None,
                    writer: None,
                },
            })));
        }

        None
    }

    #[inline]
    fn inner(&self) -> &TcpStreamInner {
        self.0.polled()
    }

    #[inline]
    fn inner_mut(&mut self) -> &mut TcpStreamInner {
        self.0.polled_mut()
    }

    unsafe extern "C" fn connect_complete(cb_arg: *mut c_void, status: i32) {
        let inner = &mut *(cb_arg as *mut TcpStreamInner);

        let next_state = match to_result!(status) {
            Ok(_) => StreamState::Connected {
                reader: None,
                writer: None,
            },
            Err(e) => Failed(e),
        };
        let prev_state = mem::replace(&mut inner.state, next_state);

        match prev_state {
            StreamState::Connecting(maybe_waker) => {
                if let Some(waker) = maybe_waker {
                    waker.wake_by_ref();
                }
            }
            _ => {
                unreachable!(
                    "connect_complete called in unexpected state: {:?}",
                    prev_state
                )
            }
        }
    }

    async fn connect_one(addr: SocketAddr, opts: &spdk_sock_opts) -> Result<Self, errors::Errno> {
        Connector(Some(TcpStream(Poller::new_in_place(move |inner| {
            let inner = inner.get_mut().write(TcpStreamInner {
                sock: ptr::null_mut(),
                state: Connecting(None),
            });

            inner.sock = unsafe {
                spdk_sock_connect_async(
                    addr.ip().as_ptr(),
                    addr.port().into(),
                    ptr::null_mut(),
                    opts as *const _ as *mut _,
                    Some(Self::connect_complete),
                    inner as *mut _ as *mut _,
                )
            };

            if inner.sock.is_null() {
                inner.state = Failed(EINVAL);
            }
        }))))
        .await
    }

    pub async fn connect<A: ToSocketAddrs>(addrs: A) -> Result<Self, errors::Errno> {
        let opts = unsafe {
            let mut opts = MaybeUninit::<spdk_sock_opts>::zeroed().assume_init();

            opts.opts_size = size_of::<spdk_sock_opts>();
            spdk_sock_get_default_opts(&mut opts as *mut _);

            opts
        };

        for addr in addrs.to_socket_addr().await? {
            if let Ok(stream) = Self::connect_one(addr, &opts).await {
                return Ok(stream);
            }
        }

        Err(EINVAL)
    }

    fn poll_connected(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), errors::Errno>> {
        if self.is_connected() {
            assert!(matches!(
                self.inner().state,
                StreamState::Connected {
                    reader: _,
                    writer: _
                }
            ));
            return Poll::Ready(Ok(()));
        }

        match self.0.polled().state {
            StreamState::Connecting(_) => Poll::Pending,
            StreamState::Failed(e) => Poll::Ready(Err(e)),
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        unsafe { spdk_sock_is_connected(self.as_raw_sock()) }
    }

    pub fn peer_addr(&self) -> Result<SocketAddr, Errno> {
        let mut addr = [0u8; 46];
        let mut port = 0u16;

        if unsafe {
            spdk_sock_getaddr(
                self.as_raw_sock(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                addr.as_mut_ptr().cast(),
                addr.len() as i32,
                addr_of_mut!(port),
            ) == 0
        } {
            return Ok(SocketAddr::new(
                CStr::from_bytes_until_nul(&addr).unwrap().into(),
                port,
            ));
        }

        Err(ENOTCONN)
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let res = to_result_size! {
            unsafe { spdk_sock_recv(self.inner().sock, addr_of_mut!(*buf) as *mut _, buf.len()) }
        };

        match res {
            Ok(s) => Poll::Ready(Ok(s)),
            Err(EAGAIN) => {
                if let StreamState::Connected { reader, writer: _ } = &mut self.inner_mut().state {
                    *reader = Some(cx.waker().clone());
                } else {
                    unreachable!(
                        "poll_read called in unexpected state: {:?}",
                        self.inner().state
                    );
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e.into())),
        }
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let res = to_result_size! {{
            let iov = IoSlice::new(buf);

            unsafe { spdk_sock_writev(self.inner().sock, addr_of!(iov) as *mut IoVec, 1) }
        }};

        match res {
            Ok(s) => Poll::Ready(Ok(s)),
            Err(EAGAIN) => {
                if let StreamState::Connected { reader: _, writer } = &mut self.inner_mut().state {
                    *writer = Some(cx.waker().clone());
                } else {
                    unreachable!(
                        "poll_read called in unexpected state: {:?}",
                        self.inner().state
                    );
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e.into())),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.inner_mut().sock as *mut _) }
        };

        match res {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(e.into())),
        }
    }
}

impl AsRawSock for TcpStream {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.inner().sock
    }
}
