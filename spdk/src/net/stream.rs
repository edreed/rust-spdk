use std::{
    future::Future,
    io::IoSlice,
    mem::{self, ManuallyDrop, MaybeUninit},
    os::raw::c_void,
    pin::Pin,
    ptr::{self, addr_of, addr_of_mut},
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use spdk_sys::{
    iovec as IoVec, spdk_sock, spdk_sock_close, spdk_sock_connect_async,
    spdk_sock_get_default_opts, spdk_sock_is_connected, spdk_sock_opts, spdk_sock_recv,
    spdk_sock_writev,
};

use crate::{
    errors::{Errno, EAGAIN, EBADF, EINVAL},
    task::{Polled, Poller},
    to_result, to_result_size,
};

use super::{Accepted, AsRawSock, SocketAddr, TcpSocketExt, TcpSocketRemote, ToSocketAddrs};

/// The state of a [`TcpStream`].
#[derive(Debug)]
enum StreamState {
    /// The stream is connecting. The optional [`Waker`] receives notification of the connection result.
    Connecting(Option<Waker>),

    /// The stream is connected. The optional [`Waker`] receives notification of read or write
    /// operation completions.
    Connected(Option<Waker>),

    /// The stream failed and is no longer functional. The [`Errno`] value describes the reason for
    /// the failure.
    Failed(Errno),
}

impl StreamState {
    /// Polls the state of the stream.
    ///
    /// # Returns
    ///
    /// Returns `true` if a waiting [`Waker`] was awakened; `false` otherwise.
    fn poll(&mut self) -> bool {
        match self {
            Self::Connecting(maybe_waker) | Self::Connected(maybe_waker) => {
                if let Some(waker) = maybe_waker.take() {
                    waker.wake();
                    return true;
                }

                false
            }
            Self::Failed(_) => false,
        }
    }

    /// Sets the state to [`Connected`].
    ///
    /// [`Connected`]: Self::Connected
    fn set_connected(&mut self) {
        let prev_state = mem::replace(self, Self::Connected(None));

        match prev_state {
            Self::Connecting(maybe_waker) => {
                if let Some(waker) = maybe_waker {
                    waker.wake();
                }
            }
            _ => unreachable!("set_connected called in unexpected state: {:?}", prev_state),
        }
    }

    /// Sets the state to [`Failed`].
    ///
    /// [`Failed`]: Self::Failed
    fn set_failed(&mut self, e: Errno) {
        let mut prev_state = mem::replace(self, Self::Failed(e));

        prev_state.poll();
    }
}

/// A future implementation for awaiting the connection of a [`TcpStream`].
#[derive(Debug)]
struct Connector(Option<TcpStream>);

impl Connector {
    /// Polls the connection state of the stream.
    fn poll_connected(&mut self, _cx: &mut Context<'_>) -> Poll<Result<TcpStream, Errno>> {
        self.0
            .as_mut()
            .map(|stream| {
                // Poll the `spdk_sock`'s connection state to update its internal state and invoke
                // the callback passed to `spdk_sock_connect_async` if the connection operation is
                // complete. The callback will update the internal `TcpStream` state with the result
                // if invoked.
                if stream.is_connected() {
                    assert!(matches!(stream.inner().state, StreamState::Connected(_)));
                    return Poll::Ready(Ok(()));
                }

                // If the `spdk_sock` was connected, the result was already returned above. We need
                // only be concerened with two possibilities now: either the connection is still
                // pending or it failed.
                match stream.inner().state {
                    StreamState::Connecting(_) => Poll::Pending,
                    StreamState::Failed(e) => Poll::Ready(Err(e)),
                    StreamState::Connected(_) => unreachable!(),
                }
            })
            .unwrap_or(Poll::Ready(Err(EBADF)))
            .map_ok(|_| self.0.take().unwrap())
    }
}

impl Future for Connector {
    type Output = Result<TcpStream, Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_connected(cx)
    }
}

/// The internal state of a [`TcpStream`].
#[derive(Debug)]
struct TcpStreamInner {
    sock: *mut spdk_sock,
    state: StreamState,
}

impl TcpStreamInner {
    /// Polls the state of the stream.
    ///
    /// # Returns
    ///
    /// Returns `true` if a waiting [`Waker`] was awakened; `false` otherwise.
    fn poll_state(&mut self) -> bool {
        self.state.poll()
    }
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
    fn poll(mut self: Pin<&mut Self>) -> bool {
        self.poll_state()
    }
}

/// A TCP connection between a local and a remote socket.
///
/// Create a `TcpStream` connected to a remote socket address by calling the
/// [`TcpStream::connect`] method. A `TcpStream` can also be created by calling the
/// [`TcpListener::accept`] method or iterating over the iterator returned by [`TcpListener::incoming`].
///
/// [`TcpListener::accept`]: crate::net::TcpListener::accept
/// [`TcpListener::incoming`]: crate::net::TcpListener::incoming
#[derive(Debug)]
pub struct TcpStream(Poller<TcpStreamInner>);

impl TcpStream {
    /// Returns an immutable reference to the internal state of the stream.
    #[inline]
    fn inner(&self) -> &TcpStreamInner {
        self.0.polled()
    }

    /// Returns an mutable reference to the internal state of the stream.
    #[inline]
    fn inner_mut(&mut self) -> &mut TcpStreamInner {
        self.0.polled_mut()
    }

    /// A callback function invoked when the connection operation is complete.
    unsafe extern "C" fn connect_complete(cb_arg: *mut c_void, status: i32) {
        let inner = &mut *(cb_arg as *mut TcpStreamInner);

        match to_result!(status) {
            Ok(_) => inner.state.set_connected(),
            Err(e) => inner.state.set_failed(e),
        };
    }

    /// Attempts to connect a stream to the specified socket address.
    async fn connect_one(addr: SocketAddr, opts: &spdk_sock_opts) -> Result<Self, Errno> {
        Connector(Some(TcpStream(Poller::new_in_place(move |inner| {
            let inner = inner.get_mut().write(TcpStreamInner {
                sock: ptr::null_mut(),
                state: StreamState::Connecting(None),
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
                inner.state = StreamState::Failed(EINVAL);
            }
        }))))
        .await
    }

    /// Creates a TCP connection to the specified socket address.
    ///
    /// If `addr` yields multiple socket addresses, `connect` will attempt to connect to each until
    /// one succeeds and returns a stream. If no address can be successfully connected,
    /// the error from the last connection attempt is returned.
    pub async fn connect<A: ToSocketAddrs>(addrs: A) -> Result<Self, Errno> {
        let opts = unsafe {
            let mut opts = MaybeUninit::<spdk_sock_opts>::zeroed().assume_init();

            opts.opts_size = size_of::<spdk_sock_opts>();
            spdk_sock_get_default_opts(&mut opts as *mut _);

            opts
        };

        let mut last_err = EINVAL;

        for addr in addrs.to_socket_addr().await? {
            match Self::connect_one(addr, &opts).await {
                Ok(stream) => return Ok(stream),
                Err(e) => last_err = e,
            }
        }

        Err(last_err)
    }

    /// Returns whether the stream is connected.
    #[inline]
    pub fn is_connected(&self) -> bool {
        unsafe { spdk_sock_is_connected(self.as_raw_sock()) }
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
                if let StreamState::Connected(waker) = &mut self.inner_mut().state {
                    *waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }

                unreachable!(
                    "poll_read called in unexpected state: {:?}",
                    self.inner().state
                );
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
        let iov = IoSlice::new(buf);
        let res = to_result_size! {
            unsafe { spdk_sock_writev(self.inner().sock, addr_of!(iov) as *mut IoVec, 1) }
        };

        match res {
            Ok(s) => Poll::Ready(Ok(s)),
            Err(EAGAIN) => {
                if let StreamState::Connected(waker) = &mut self.inner_mut().state {
                    *waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }

                unreachable!(
                    "poll_write called in unexpected state: {:?}",
                    self.inner().state
                );
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

impl TcpSocketExt for TcpStream {}

impl TcpSocketRemote for TcpStream {}

impl From<Accepted> for TcpStream {
    fn from(value: Accepted) -> Self {
        let value = ManuallyDrop::new(value);

        assert!(!value.0.is_null());

        Self(Poller::new(TcpStreamInner {
            sock: value.0,
            state: StreamState::Connected(None),
        }))
    }
}
