use std::{mem, pin::Pin, ptr::{self, NonNull}, task::{Context, Poll, Waker}};

use errno::Errno;
use futures::{Stream, StreamExt};
use spdk_sys::{spdk_sock, spdk_sock_accept, spdk_sock_listen};

use crate::{errors::{self, EAGAIN, EBADF, EINVAL}, task::{Polled, Poller}};

use super::{AsRawSock, SocketAddr, TcpStream, ToSocketAddrs};

enum AcceptorState {
    Idle,
    Waiting(Waker),
    Available(Result<(TcpStream, SocketAddr), Errno>)
}

impl AcceptorState {
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

struct Acceptor<'a> {
    listener: &'a TcpListener,
    state: AcceptorState
}

impl<'a> Polled for Acceptor<'a> {
    fn poll(&mut self) -> bool {
        if let AcceptorState::Waiting(_) = self.state {
            if let Poll::Ready(res) = self.listener.poll_accept() {
                if let AcceptorState::Waiting(waker) = self.state.replace(AcceptorState::Available(res)) {
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

pub struct Incoming<'a> {
    poller: Poller<Acceptor<'a>>
}

impl<'a> Incoming<'a> {
    fn new(listener: &'a TcpListener) -> Self {
        Incoming {
            poller: Poller::new(Acceptor {
                listener,
                state: AcceptorState::Idle,
            })
        }
    }
}

impl<'a> Stream for Incoming<'a> {
    type Item = Result<(TcpStream, SocketAddr), Errno>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().poller.polled_mut().state.poll(cx) {
            AcceptorState::Idle | AcceptorState::Waiting(_) => Poll::Pending,
            AcceptorState::Available(res) => Poll::Ready(Some(res)),
        }
    }
}

pub struct TcpListener(NonNull<spdk_sock>);

impl TcpListener {
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> Result<Self, errors::Errno> {
        for addr in addrs.to_socket_addr().await? {
            let listener = unsafe {
                spdk_sock_listen(
                    addr.ip().as_ptr(),
                    addr.port() as i32,
                    ptr::null())
            };

            match NonNull::new(listener) {
                Some(sock) => return Ok(Self(sock)),
                None => continue,
            }
        }

        Err(EINVAL)
    }

    fn poll_accept(&self) -> Poll<Result<(TcpStream, SocketAddr), Errno>> {
        match TcpStream::from_ptr(unsafe { spdk_sock_accept(self.0.as_ptr()) }) {
            Some(socket) => {
                let peer = socket.peer_addr()?;

                Poll::Ready(Ok((socket, peer)))
            },
            None => {
                let err = errno::errno();

                if err == EAGAIN {
                    return Poll::Pending;
                }

                Poll::Ready(Err(err))
            },
        }
    }

    pub fn incoming(&self) -> Incoming {
        Incoming::new(self)
    }

    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), Errno> {
        self.incoming().next().await.unwrap_or(Err(EBADF))
    }
}

impl AsRawSock for TcpListener {
    fn as_raw_sock(&self) -> super::RawSock {
        self.0.as_ptr()
    }
}