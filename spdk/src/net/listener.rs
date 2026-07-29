use std::{
    pin::Pin,
    ptr,
    task::{Context, Poll, Waker},
};

use futures::{Stream, StreamExt};
use spdk_sys::{spdk_sock, spdk_sock_accept, spdk_sock_close, spdk_sock_listen};

use crate::{
    errors::{self, EAGAIN, EBADF, EINVAL, Errno, errno},
    task::Polled,
    to_result,
};

use super::{
    AsRawSock, SocketAddr, TcpSocketExt, TcpSocketRemote, ToSocketAddrs, bind_polled_listener,
};

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
    listener: &'a mut RawTcpListener,
}

impl<'a> Incoming<'a> {
    /// Creates new `Incoming` instance for the specified [`TcpListener`].
    fn new(listener: &'a mut RawTcpListener) -> Self {
        Self { listener }
    }
}

impl<'a> Stream for Incoming<'a> {
    type Item = Result<Accepted, Errno>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        unsafe {
            Pin::map_unchecked_mut(self, |s| s.listener)
                .poll_accept(cx)
                .map(Option::Some)
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

#[derive(Debug)]
pub(crate) struct TcpListenerSocket {
    sock: *mut spdk_sock,
    waker: Option<Waker>,
}

impl TcpListenerSocket {
    pub(crate) fn bind(addr: SocketAddr) -> Result<Self, Errno> {
        let sock = unsafe { spdk_sock_listen(addr.ip().as_ptr(), addr.port() as i32, ptr::null()) };

        if !sock.is_null() {
            return Ok(Self { sock, waker: None });
        }

        Err(EINVAL)
    }

    pub(crate) fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Accepted, Errno>> {
        let accepted = unsafe { spdk_sock_accept(self.sock) };

        if !accepted.is_null() {
            return Poll::Ready(Ok(Accepted(accepted)));
        }

        let err = errno();

        if err == EAGAIN {
            self.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        Poll::Ready(Err(err))
    }
}

impl AsRawSock for TcpListenerSocket {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.sock
    }
}

impl Polled for TcpListenerSocket {
    fn poll(mut self: Pin<&mut Self>) -> bool {
        if let Some(waker) = self.waker.take() {
            waker.wake();
            return true;
        }

        false
    }
}

impl Drop for TcpListenerSocket {
    fn drop(&mut self) {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.sock as *mut _) }
        };

        res.map_err(|e| if e == EBADF { Ok(()) } else { Err(e) })
            .expect("socket closed");
    }
}

#[derive(Debug)]
pub(crate) struct RawTcpListenerVtable {
    pub(crate) as_raw_sock: unsafe fn(*const ()) -> *mut spdk_sock,
    pub(crate) poll_accept: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<Accepted, Errno>>,
    pub(crate) drop: unsafe fn(*const ()),
}

#[derive(Debug)]
pub(crate) struct RawTcpListener {
    data: *const (),
    vtable: &'static RawTcpListenerVtable,
}

impl RawTcpListener {
    pub(crate) fn new(data: *const (), vtable: &'static RawTcpListenerVtable) -> Self {
        Self { data, vtable }
    }

    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Accepted, Errno>> {
        unsafe { (self.vtable.poll_accept)(self.data, cx) }
    }
}

impl AsRawSock for RawTcpListener {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        unsafe { (self.vtable.as_raw_sock)(self.data) }
    }
}

impl Drop for RawTcpListener {
    fn drop(&mut self) {
        unsafe { (self.vtable.drop)(self.data) };
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
pub struct TcpListener(RawTcpListener);

impl TcpListener {
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
            match bind_polled_listener(addr).map(TcpListener).ok() {
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
        Incoming::new(&mut self.0)
    }
}

impl AsRawSock for TcpListener {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.0.as_raw_sock()
    }
}

impl TcpSocketExt for TcpListener {}
