use std::{
    future::Future,
    io::IoSlice,
    mem::{ManuallyDrop, MaybeUninit},
    os::raw::c_void,
    pin::Pin,
    ptr::{self, addr_of, addr_of_mut},
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use spdk_sys::{
    iovec as IoVec, spdk_sock, spdk_sock_close, spdk_sock_connect_async, spdk_sock_flush,
    spdk_sock_get_default_opts, spdk_sock_is_connected, spdk_sock_opts, spdk_sock_recv,
    spdk_sock_writev,
};

use crate::{
    errors::{EAGAIN, EBADF, EINVAL, Errno},
    task::Polled,
    to_result, to_result_size,
};

use super::{
    Accepted, AsRawSock, SocketAddr, TcpSocketExt, TcpSocketRemote, ToSocketAddrs,
    connect_polled_stream, new_polled_stream,
};

/// A future implementation for awaiting the connection of a [`TcpStream`].
#[derive(Debug)]
struct Connector(Option<RawTcpStream>);

impl Connector {
    /// Polls the connection state of the stream.
    fn poll_connected(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<TcpStream, Errno>> {
        unsafe { Pin::map_unchecked_mut(self.as_mut(), |s| &mut s.0) }
            .as_pin_mut()
            .map(|stream| RawTcpStream::poll_connected(stream, cx))
            .unwrap_or(Poll::Ready(Err(EBADF)))
            .map_ok(|_| TcpStream(self.0.take().unwrap()))
    }
}

impl Future for Connector {
    type Output = Result<TcpStream, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_connected(cx)
    }
}

#[derive(Debug)]
pub(crate) struct TcpStreamSocket {
    sock: *mut spdk_sock,
    waker: Option<Waker>,
}

impl TcpStreamSocket {
    pub(crate) fn new(sock: *mut spdk_sock) -> Self {
        Self { sock, waker: None }
    }

    /// A callback function invoked when the connection operation is complete.
    unsafe extern "C" fn connect_complete(cb_arg: *mut c_void, _status: i32) {
        let inner = unsafe { &mut *(cb_arg as *mut TcpStreamSocket) };

        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }

    /// Creates a TCP connection to the specified socket address, initializing the internal state of
    /// a [`TcpStreamSocket`} in-place.
    pub(crate) fn connect_in_place(
        this: Pin<&mut MaybeUninit<TcpStreamSocket>>,
        addr: SocketAddr,
        opts: &spdk_sock_opts,
    ) {
        let this = this.get_mut().write(TcpStreamSocket {
            sock: ptr::null_mut(),
            waker: None,
        });

        this.sock = unsafe {
            spdk_sock_connect_async(
                addr.ip().as_ptr(),
                addr.port().into(),
                ptr::null_mut(),
                opts as *const _ as *mut _,
                Some(Self::connect_complete),
                this as *mut _ as *mut _,
            )
        };
    }

    pub(crate) fn poll_connected(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        let res = to_result!(unsafe { spdk_sock_flush(self.sock) });

        match res {
            Ok(()) => Poll::Ready(Ok(())),
            Err(EAGAIN) => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    pub(crate) fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Errno>> {
        let res = to_result_size!(unsafe {
            spdk_sock_recv(self.sock, addr_of_mut!(*buf) as *mut _, buf.len())
        });

        match res {
            Ok(bytes_read) => Poll::Ready(Ok(bytes_read)),
            Err(EAGAIN) => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    pub(crate) fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Errno>> {
        let iov = IoSlice::new(buf);
        let res =
            to_result_size!(unsafe { spdk_sock_writev(self.sock, addr_of!(iov) as *mut IoVec, 1) });

        match res {
            Ok(bytes_written) => Poll::Ready(Ok(bytes_written)),
            Err(EAGAIN) => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    pub(crate) fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        Poll::Ready(Ok(()))
    }

    pub(crate) fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        Poll::Ready(to_result!(unsafe {
            spdk_sock_close(&mut self.sock as *mut _)
        }))
    }
}

impl AsRawSock for TcpStreamSocket {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.sock
    }
}

impl Polled for TcpStreamSocket {
    fn poll(mut self: Pin<&mut Self>) -> bool {
        if let Some(waker) = self.waker.take() {
            waker.wake();
            return true;
        }

        false
    }
}

impl Drop for TcpStreamSocket {
    fn drop(&mut self) {
        let res = to_result! {
            unsafe { spdk_sock_close(&mut self.sock as *mut _) }
        };

        res.map_err(|e| if e == EBADF { Ok(()) } else { Err(e) })
            .expect("socket closed");
    }
}

#[derive(Debug)]
pub(crate) struct RawTcpStreamVtable {
    pub(crate) as_raw_sock: unsafe fn(*const ()) -> *mut spdk_sock,
    pub(crate) poll_connected: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,
    #[allow(clippy::type_complexity)]
    pub(crate) poll_read:
        unsafe fn(*const (), &mut Context<'_>, &mut [u8]) -> Poll<Result<usize, Errno>>,
    #[allow(clippy::type_complexity)]
    pub(crate) poll_write:
        unsafe fn(*const (), &mut Context<'_>, &[u8]) -> Poll<Result<usize, Errno>>,
    pub(crate) poll_flush: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,
    pub(crate) poll_close: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,
    pub(crate) drop: unsafe fn(*const ()),
}

#[derive(Debug)]
pub(crate) struct RawTcpStream {
    data: *const (),
    vtable: &'static RawTcpStreamVtable,
}

impl RawTcpStream {
    pub(crate) fn new(data: *const (), vtable: &'static RawTcpStreamVtable) -> Self {
        Self { data, vtable }
    }

    fn poll_connected(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        unsafe { (self.vtable.poll_connected)(self.data, cx) }
    }

    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_read)(self.data, cx, buf) }
    }

    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_write)(self.data, cx, buf) }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        unsafe { (self.vtable.poll_flush)(self.data, cx) }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        unsafe { (self.vtable.poll_close)(self.data, cx) }
    }
}

impl AsRawSock for RawTcpStream {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        unsafe { (self.vtable.as_raw_sock)(self.data) }
    }
}

impl Drop for RawTcpStream {
    fn drop(&mut self) {
        unsafe { (self.vtable.drop)(self.data) }
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
pub struct TcpStream(RawTcpStream);

impl TcpStream {
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
            match Connector(Some(connect_polled_stream(addr, &opts))).await {
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
        Pin::new(&mut self.0).poll_read(cx, buf).map_err(Into::into)
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0)
            .poll_write(cx, buf)
            .map_err(Into::into)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx).map_err(Into::into)
    }
}

impl AsRawSock for TcpStream {
    fn as_raw_sock(&self) -> *mut spdk_sock {
        self.0.as_raw_sock()
    }
}

impl TcpSocketExt for TcpStream {}

impl TcpSocketRemote for TcpStream {}

impl From<Accepted> for TcpStream {
    fn from(value: Accepted) -> Self {
        let value = ManuallyDrop::new(value);

        assert!(!value.0.is_null());

        Self(new_polled_stream(value.0))
    }
}
