//! The implementation of a TCP connection between local and remote sockets.
use std::{
    future::Future,
    io::{IoSlice, IoSliceMut},
    mem::MaybeUninit,
    os::raw::c_void,
    pin::Pin,
    ptr::{self, addr_of, addr_of_mut},
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use spdk_sys::{
    iovec as IoVec, spdk_sock, spdk_sock_close, spdk_sock_connect_async, spdk_sock_flush,
    spdk_sock_get_default_opts, spdk_sock_is_connected, spdk_sock_opts, spdk_sock_readv,
    spdk_sock_recv, spdk_sock_writev,
};

use crate::{
    errors::{EAGAIN, EBADF, EINPROGRESS, EINVAL, Errno, SUCCESS},
    task::Polled,
    to_result, to_result_size,
};

use super::{
    AsRawSock, SocketAddr, SocketGroupEvent, TcpSocketExt, TcpSocketRemote, ToSocketAddrs,
    connect_polled_stream,
};

/// A future implementation for awaiting the connection of a [`TcpStream`].
#[derive(Debug)]
pub(crate) struct Connector(Option<RawTcpStream>);

impl Connector {
    /// Creates a new `Connector` instance for specified [`TcpStream`].
    pub(crate) fn new(stream: RawTcpStream) -> Self {
        Self(Some(stream))
    }

    /// Polls the connection state of the stream.
    ///
    /// # Returns
    ///
    /// This method returns `Poll::Ready(Ok())` if the stream is connected, `Poll::Pending` if the
    /// outgoing connection is pending, and `Poll::Ready(Err(`[`Errno`]`))` if the connection failed.
    fn poll_connected(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<RawTcpStream, Errno>> {
        // SAFETY: We're mapping to a field of a pinned object.
        unsafe { Pin::map_unchecked_mut(self.as_mut(), |s| &mut s.0) }
            .as_pin_mut()
            .map(|stream| stream.poll_connected(cx))
            .unwrap_or(Poll::Ready(Err(EBADF)))
            .map_ok(|_| self.0.take().unwrap())
    }
}

impl Future for Connector {
    type Output = Result<TcpStream, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_connected(cx).map_ok(TcpStream::new)
    }
}

/// The internal socket implementation for a [`TcpStream`].
#[derive(Debug)]
pub(crate) struct TcpStreamSocket {
    /// A pointer to an `spdk_sock` returned by the [`spdk_sock_connect_async`] or [`spdk_sock_accept`] functions.
    ///
    /// [`spdk_sock_accept`]: spdk_sys::spdk_sock_accept
    sock: *mut spdk_sock,

    /// The [`Waker`] awaiting an I/O operation.
    waker: Option<Waker>,

    /// The connection status.
    conn_status: Errno,
}

impl TcpStreamSocket {
    /// Creates a new `TcpStreamSocket` instance from an `spdk_sock` returned by the
    /// [`spdk_sock_connect_async`] or [`spdk_sock_accept`] functions.
    ///
    /// [`spdk_sock_accept`]: spdk_sys::spdk_sock_accept
    pub(crate) fn new(sock: *mut spdk_sock) -> Self {
        Self {
            sock,
            waker: None,
            conn_status: SUCCESS,
        }
    }

    /// A callback function invoked when the connection operation is complete.
    unsafe extern "C" fn connect_complete(cb_arg: *mut c_void, status: i32) {
        let inner = unsafe { &mut *(cb_arg as *mut TcpStreamSocket) };

        inner.conn_status = Errno::new(-status);

        if let Some(waker) = inner.waker.take() {
            waker.wake();
        }
    }

    /// Creates a TCP connection to the specified socket address, initializing the
    /// [`TcpStreamSocket`] in-place.
    pub(crate) fn connect_in_place(
        this: Pin<&mut MaybeUninit<TcpStreamSocket>>,
        addr: SocketAddr,
        opts: &spdk_sock_opts,
    ) {
        let this = this.get_mut().write(TcpStreamSocket {
            sock: ptr::null_mut(),
            waker: None,
            conn_status: EINPROGRESS,
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

    /// Polls the connection state of the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// This method returns `Poll::Ready(Ok())` if the stream is connected, `Poll::Pending` if the
    /// outgoing connection is pending, and `Poll::Ready(Err(`[`Errno`]`))` if the connection failed.
    pub(crate) fn poll_connected(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        // Call `spdk_sock_flush` to get an indication whether the connection is in-progress,
        // complete or failed.
        let res = to_result!(unsafe { spdk_sock_flush(self.sock) });

        match res {
            Ok(()) => Poll::Ready(Ok(())),
            Err(EAGAIN) => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(_) => Poll::Ready(Err(self.conn_status)),
        }
    }

    /// Attempts to read from the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
    /// available and `Poll::Pending` if no data was available.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
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

    /// Attempts to read from the [`TcpStreamSocket`] into multiple buffers.
    ///
    /// # Returns
    ///
    /// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
    /// available and `Poll::Pending` if no data was available.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[``Errno`]`))`.
    pub(crate) fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut],
    ) -> Poll<Result<usize, Errno>> {
        let res = to_result_size!(unsafe {
            spdk_sock_readv(self.sock, bufs as *mut _ as *mut IoVec, bufs.len() as i32)
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

    /// Attempts to write to the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data was
    /// written and `Poll::Pending` data cannot currently be written.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
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

    /// Attempts to write to the [`TcpStreamSocket`] from multiple buffers.
    ///
    /// # Returns
    ///
    /// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data was
    /// written and `Poll::Pending` data cannot currently be written.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[``Errno`]`))`.
    pub(crate) fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice],
    ) -> Poll<Result<usize, Errno>> {
        let res = to_result_size!(unsafe {
            spdk_sock_writev(self.sock, bufs as *const _ as *mut IoVec, bufs.len() as i32)
        });

        match res {
            Ok(bytes_written) => Poll::Ready(Ok(bytes_written)),
            Err(EAGAIN) => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    /// Attempts to flush buffered data from the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// The SPDK does not expose a means explicitly flush the buffer data in the socket, so this
    /// method always returns `Poll::Ready(Ok())`.
    pub(crate) fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Errno>> {
        Poll::Ready(Ok(()))
    }

    /// Attempts to close the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// This method returns `Poll::Ready(Ok())` if the socket was successfully closed, and
    /// `Poll::Ready(Err(`[`Errno`]`))` otherwise.
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

impl SocketGroupEvent for TcpStreamSocket {
    fn handle_event(mut self: Pin<&mut Self>) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
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

/// A virtual function pointer table (vtable) that specifies the methods that can be invoked on a
/// [`RawTcpStream`] instance.
#[derive(Debug)]
pub(crate) struct RawTcpStreamVtable {
    /// Returns the [`RawTcpStream`]'s raw `spdk_sock` pointer.
    pub(crate) as_raw_sock: unsafe fn(*const ()) -> *mut spdk_sock,

    /// Polls the connection state of the [`RawTcpStream`].
    pub(crate) poll_connected: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,

    /// Attempts to read from the [`RawTcpStream`].
    #[allow(clippy::type_complexity)]
    pub(crate) poll_read:
        unsafe fn(*const (), &mut Context<'_>, &mut [u8]) -> Poll<Result<usize, Errno>>,

    /// Attempts to read from the [`RawTcpStream`] into multiple buffers.
    #[allow(clippy::type_complexity)]
    pub(crate) poll_read_vectored:
        unsafe fn(*const (), &mut Context<'_>, &mut [IoSliceMut]) -> Poll<Result<usize, Errno>>,

    /// Attempts to write to the [`RawTcpStream`].
    #[allow(clippy::type_complexity)]
    pub(crate) poll_write:
        unsafe fn(*const (), &mut Context<'_>, &[u8]) -> Poll<Result<usize, Errno>>,

    /// Attempts to write to the [`RawTcpStream`] from multiple buffers.
    #[allow(clippy::type_complexity)]
    pub(crate) poll_write_vectored:
        unsafe fn(*const (), &mut Context<'_>, &[IoSlice]) -> Poll<Result<usize, Errno>>,

    /// Attempts to flush buffered data in the [`RawTcpStream`].
    pub(crate) poll_flush: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,

    /// Attempts to close the [`RawTcpStream`].
    pub(crate) poll_close: unsafe fn(*const (), &mut Context<'_>) -> Poll<Result<(), Errno>>,

    /// Drops the [`RawTcpStream`].
    pub(crate) drop: unsafe fn(*const ()),
}

/// Enables dynamic dispatch to a TCP socket stream implementation.
#[derive(Debug)]
pub(crate) struct RawTcpStream {
    data: *const (),
    vtable: &'static RawTcpStreamVtable,
}

impl RawTcpStream {
    /// Creates a new `RawTcpStream` instance with the specified virtual function table.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `vtable` is correct for the `data` pointer.
    pub(crate) unsafe fn new(data: *const (), vtable: &'static RawTcpStreamVtable) -> Self {
        Self { data, vtable }
    }

    /// Polls the connection state of the TCP stream.
    ///
    /// # Returns
    ///
    /// This method returns `Poll::Ready(Ok())` if the stream is connected, `Poll::Pending` if the
    /// outgoing connection is pending, and `Poll::Ready(Err(`[`Errno`]`))` if the connection failed.
    fn poll_connected(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        unsafe { (self.vtable.poll_connected)(self.data, cx) }
    }

    /// Attempts to read from the [`TcpStreamSocket`].
    ///
    /// # Returns
    ///
    /// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
    /// available and `Poll::Pending` if no data was available.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_read)(self.data, cx, buf) }
    }

    /// Attempts to read from the [`TcpStreamSocket`] into multiple buffers.
    ///
    /// # Returns
    ///
    /// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
    /// available and `Poll::Pending` if no data was available.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[``Errno`]`))`.
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_read_vectored)(self.data, cx, bufs) }
    }

    /// Attempts to write to the TCP stream.
    ///
    /// # Returns
    ///
    /// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data was
    /// written and `Poll::Pending` data cannot currently be written.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_write)(self.data, cx, buf) }
    }

    /// Attempts to write to the TCP stream from multiple buffers.
    ///
    /// # Returns
    ///
    /// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data
    /// was written and `Poll::Pending` data cannot currently be written.
    ///
    /// If the connection has failed, this method returns `Poll::Ready(Err(`[``Errno`]`))`.
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice],
    ) -> Poll<Result<usize, Errno>> {
        unsafe { (self.vtable.poll_write_vectored)(self.data, cx, bufs) }
    }

    /// Attempts to flush buffered data from the TCP stream.
    ///
    /// # Returns
    ///
    /// The SPDK does not expose a means explicitly flush the buffer data in the socket, so this
    /// method always returns `Poll::Ready(Ok())`.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
        unsafe { (self.vtable.poll_flush)(self.data, cx) }
    }

    /// Attempts to close the TCP stream.
    ///
    /// # Returns
    ///
    /// This method returns `Poll::Ready(Ok())` if the socket was successfully closed, and
    /// `Poll::Ready(Err(`[`Errno`]`))` otherwise.
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
    pub(crate) fn new(stream: RawTcpStream) -> Self {
        Self(stream)
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
            match connect_polled_stream(addr, &opts).await {
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

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [std::io::IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0)
            .poll_read_vectored(cx, bufs)
            .map_err(Into::into)
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

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0)
            .poll_write_vectored(cx, bufs)
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
