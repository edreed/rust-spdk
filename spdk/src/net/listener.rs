use std::{
    mem,
    pin::Pin,
    ptr,
    task::{Context, Poll, Waker},
};

use futures::{Stream, StreamExt};
use spdk_sys::{spdk_sock, spdk_sock_accept, spdk_sock_close, spdk_sock_listen};

use crate::{
    errors::{self, errno, Errno, EAGAIN, EBADF, EINVAL},
    task::{Polled, Poller},
    to_result,
};

use super::{AsRawSock, SocketAddr, TcpStream, ToSocketAddrs};

enum ListenerState {
    Idle,
    Waiting(Waker),
    Available(Result<TcpStream, Errno>),
}

impl ListenerState {
    fn replace(&mut self, value: Self) -> Self {
        mem::replace(&mut *self, value)
    }

    fn poll(&mut self, cx: &Context<'_>) -> Self {
        match self {
            Self::Idle => self.replace(Self::Waiting(cx.waker().clone())),
            Self::Waiting(waker) => Self::Waiting(waker.clone()),
            Self::Available(_) => self.replace(Self::Idle),
        }
    }
}

pub struct Incoming<'a> {
    listener: &'a mut TcpListener,
}

impl<'a> Incoming<'a> {
    fn new(listener: &'a mut TcpListener) -> Self {
        Self { listener }
    }
}

impl<'a> Stream for Incoming<'a> {
    type Item = Result<TcpStream, Errno>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().listener.0.polled_mut().state.poll(cx) {
            ListenerState::Idle | ListenerState::Waiting(_) => Poll::Pending,
            ListenerState::Available(res) => Poll::Ready(Some(res)),
        }
    }
}

struct TcpListenerInner {
    sock: *mut spdk_sock,
    state: ListenerState,
}

impl TcpListenerInner {
    fn poll_accept(&self) -> Poll<Result<TcpStream, Errno>> {
        TcpStream::from_ptr(unsafe { spdk_sock_accept(self.sock) }).map_or_else(
            || {
                let err = errno();

                if err == EAGAIN {
                    return Poll::Pending;
                }

                Poll::Ready(Err(err))
            },
            |stream| Poll::Ready(Ok(stream)),
        )
    }
}

impl Drop for TcpListenerInner {
    fn drop(&mut self) {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.sock as *mut _) }
        };

        res.map_err(|e| if e == EBADF { Ok(()) } else { Err(e) })
            .expect("socket closed");
    }
}

impl Polled for TcpListenerInner {
    fn poll(mut self: Pin<&mut Self>) -> bool {
        if let ListenerState::Waiting(_) = self.state {
            if let Poll::Ready(res) = self.poll_accept() {
                if let ListenerState::Waiting(waker) =
                    self.state.replace(ListenerState::Available(res))
                {
                    waker.wake();
                } else {
                    unreachable!();
                }

                return true;
            }
        }

        false
    }
}

pub struct TcpListener(Poller<TcpListenerInner>);

impl TcpListener {
    fn bind_one(addr: SocketAddr) -> Option<Self> {
        let raw_sock =
            unsafe { spdk_sock_listen(addr.ip().as_ptr(), addr.port() as i32, ptr::null()) };

        if !raw_sock.is_null() {
            return Some(Self(Poller::new(TcpListenerInner {
                sock: raw_sock,
                state: ListenerState::Idle,
            })));
        }

        None
    }

    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> Result<Self, errors::Errno> {
        for addr in addrs.to_socket_addr().await? {
            match Self::bind_one(addr) {
                Some(listener) => return Ok(listener),
                None => continue,
            }
        }

        Err(EINVAL)
    }

    pub fn incoming(&mut self) -> Incoming<'_> {
        Incoming::new(self)
    }

    pub async fn accept(&mut self) -> Result<TcpStream, Errno> {
        self.incoming().next().await.unwrap_or(Err(EBADF))
    }
}

impl AsRawSock for TcpListener {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.0.polled().sock
    }
}
