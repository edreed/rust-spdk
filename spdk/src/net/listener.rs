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

use super::{AsRawSock, SocketAddr, TcpSocketExt, TcpSocketRemote, ToSocketAddrs};

/// The state of a [`TcpListener`].
#[derive(Debug)]
enum ListenerState {
    /// The listener is idle, i.e. it has not been bound with a [`Waker`] to receive connection
    /// result notifications.
    Idle,

    /// The listener has been bound with a [`Waker`] to receive connection result notifications.
    Waiting(Waker),

    /// A result is available.
    Available(Result<Accepted, Errno>),
}

impl ListenerState {
    /// Polls the state of the listener, advancing to the next state if possible.
    fn poll(&mut self, cx: &Context<'_>) -> Self {
        match self {
            Self::Idle => mem::replace(self, Self::Waiting(cx.waker().clone())),
            Self::Waiting(waker) => Self::Waiting(waker.clone()),
            Self::Available(_) => mem::replace(self, Self::Idle),
        }
    }

    /// Sets the state of the listener with an available result.
    fn set_available(&mut self, res: Result<Accepted, Errno>) -> Self {
        match self {
            Self::Idle | Self::Waiting(_) => mem::replace(self, Self::Available(res)),
            _ => unreachable!("set_available called in unexpected state: {:?}", self),
        }
    }
}

/// An iterator created by calling the [`TcpListener::incoming()`] method that infinitely accepts
/// connections asynchronously on a [`TcpListener`].
///
/// Since the stream is infinite, awaiting the next connection will never result in `None`.
///
/// The elements returned by this iterator are intermediary [`Accepted`] instances that enable
/// connected sockets to be sent to another [`Thread`]s. See the [`accept`] method for more
/// information.
///
/// [`accept`]: TcpListener::accept
/// [`Thread`]: crate::thread::Thread
pub struct Incoming<'a> {
    listener: &'a mut TcpListenerInner,
}

impl<'a> Incoming<'a> {
    /// Creates new `Incoming` instance for the specified [`TcpListener`].
    fn new(listener: &'a mut TcpListener) -> Self {
        Self {
            listener: listener.inner_mut(),
        }
    }
}

impl<'a> Stream for Incoming<'a> {
    type Item = Result<Accepted, Errno>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().listener.poll_state(cx) {
            ListenerState::Idle | ListenerState::Waiting(_) => Poll::Pending,
            ListenerState::Available(res) => Poll::Ready(Some(res)),
        }
    }
}

/// Wraps a TCP socket connection accepted by a [`TcpListener`].
///
/// This is an intermediary type returned by the [`TcpListener::accept`] method and iterator
/// returned by the [`TcpListener::incoming`] method. It enables connected sockets to be sent to
/// another [`Thread`]. See the [`TcpListener::accept`] method for more information.
///
/// [`Thread`]: crate::thread::Thread
#[derive(Debug)]
pub struct Accepted(pub(crate) *mut spdk_sock);

unsafe impl Send for Accepted {}

impl Drop for Accepted {
    fn drop(&mut self) {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.0 as *mut _) }
        };

        res.map_err(|e| if e == EBADF { Ok(()) } else { Err(e) })
            .expect("socket closed");
    }
}

impl AsRawSock for Accepted {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.0
    }
}

impl TcpSocketExt for Accepted {}

impl TcpSocketRemote for Accepted {}

/// The internal state of a [`TcpListener`].
#[derive(Debug)]
struct TcpListenerInner {
    sock: *mut spdk_sock,
    state: ListenerState,
}

impl TcpListenerInner {
    /// Attempts to accept a new incoming connection.
    ///
    /// # Returns
    ///
    /// Returns `Poll::Ready(Ok(Accepted(_)))` if there is a new incoming connection,
    /// `Poll::Pending` if there is none, and `Poll::Ready(Err(Errno))` if the listener failed to
    /// bind.
    fn poll_accept(&self) -> Poll<Result<Accepted, Errno>> {
        let sock = unsafe { spdk_sock_accept(self.sock) };

        if !sock.is_null() {
            return Poll::Ready(Ok(Accepted(sock)));
        }

        let err = errno();

        if err == EAGAIN {
            return Poll::Pending;
        }

        Poll::Ready(Err(err))
    }

    /// Polls the state of the listener, advancing to the next state if possible.
    fn poll_state(&mut self, cx: &mut Context<'_>) -> ListenerState {
        self.state.poll(cx)
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
        if let Poll::Ready(res) = self.poll_accept() {
            if let ListenerState::Waiting(waker) = self.state.set_available(res) {
                waker.wake();
            }

            return true;
        }

        false
    }
}

/// A TCP socket server, listening for incoming connections.
///
/// Create a `TcpListener` bound to a local socket address by calling the [`TcpListener::bind`]
/// method to listen for new incoming connections. A new connection is accepted by calling
/// [`accept`] or by iterating over the [`Incoming`] iterator returned by [`incoming`]. New
/// connections are represented by [`TcpStream`].
///
/// [`accept`]: TcpListener::accept
/// [`incoming`]: TcpListener::incoming
/// [`TcpStream`]: super::TcpStream
#[derive(Debug)]
pub struct TcpListener(Poller<TcpListenerInner>);

impl TcpListener {
    /// Returns a mutable reference to the inner listener state.
    fn inner_mut(&mut self) -> &mut TcpListenerInner {
        self.0.polled_mut()
    }

    /// Attempts to bind a listener to the specified socket address.
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

    /// Creates a new [`TcpListener`] bound to the specified socket address.
    ///
    /// If the port number of the socket address is omitted or `0`, the operating system will assign
    /// a port to this listener. The allocated port can be discovered by calling the
    /// [`TcpSocketExt::local_addr()`] method.
    ///
    /// If `addr` yields multiple socket address, `bind` will attempt listen on each until one
    /// succeeds and returns a listener. If no address can be successfully bound, `Err(EINVAL)` is
    /// returned.
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> Result<Self, errors::Errno> {
        for addr in addrs.to_socket_addr().await? {
            match Self::bind_one(addr) {
                Some(listener) => return Ok(listener),
                None => continue,
            }
        }

        Err(EINVAL)
    }

    /// Accepts a new incoming connection from this listener.
    ///
    /// Since a [`TcpStream`] is bound to the [`Thread`] on which is was created, `accept` returns
    /// an [`Accepted`] intermediate type that is [`Send`]. This allows the connected socket to be
    /// sent to another thread for use. To convert the `Accepted` instance into a `TcpStream`, use
    /// [`into`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use spdk::net::{TcpListener, TcpStream};
    ///
    /// let listener = TcpListener::bind("127.0.0.1:8080").await.expect("listener bound");
    /// let remote: TcpStream = listener.accept().await.expect("remote connected").into();
    /// ```
    ///
    /// [`Thread`]: crate::thread::Thread
    /// [`into`]: std::convert::Into::into
    /// [`TcpStream`]: super::TcpStream
    pub async fn accept(&mut self) -> Result<Accepted, Errno> {
        self.incoming().next().await.unwrap_or(Err(EBADF))
    }

    /// Returns an iterator over the connections being received on this listener.
    ///
    /// Iterating over this stream is the equivalent of calling [`accept`] in a loop. The stream of
    /// connections is infinite, i.e. awaiting the next connection will never return `None`.
    ///
    /// The elements returned by this iterator are intermediary [`Accepted`] instances that enable
    /// connected sockets to be sent to another [`Thread`]. See the [`accept`] method for more
    /// information.
    ///
    /// [`accept`]: TcpListener::accept
    /// [`Thread`]: crate::thread::Thread
    pub fn incoming(&mut self) -> Incoming<'_> {
        Incoming::new(self)
    }
}

impl AsRawSock for TcpListener {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.0.polled().sock
    }
}

impl TcpSocketExt for TcpListener {}
