//! Contains the SPDK poller based TCP listener and stream implementations.
use std::{
    mem::ManuallyDrop,
    pin::Pin,
    task::{Context, Poll},
};

use spdk_sys::{spdk_sock, spdk_sock_opts};

use crate::{errors::Errno, task::Poller};

use super::{
    Accepted, AsRawSock, Connector, RawTcpListener, RawTcpListenerVtable, RawTcpStream,
    RawTcpStreamVtable, SocketAddr, TcpListener, TcpListenerSocket, TcpStream, TcpStreamSocket,
};

/// Returns the [`RawTcpListener`]'s raw `spdk_sock` pointer.
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpListenerSocket`]`>` instance.
fn listener_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });

    this.polled().as_raw_sock()
}

/// Polls the [`RawTcpListener`] for an incoming connection.
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpListenerSocket`]`>` instance.
///
/// # Returns
///
/// If an incoming connection is available, this function returns `Poll<Ok(`[`Accepted`]`)>`. See
/// the [`TcpListener::accept`] for details on converting the `Accepted` instance into a
/// [`TcpStream`].
fn listener_poll_accept(data: *const (), cx: &mut Context<'_>) -> Poll<Result<Accepted, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });

    Pin::new(this.polled_mut()).poll_accept(cx)
}

/// Drops the [`RawTcpListener`].
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpListenerSocket`]`>` instance.
fn listener_drop(data: *const ()) {
    drop(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });
}

/// Returns the [`RawTcpListenerVtable`] used by a [`RawTcpListener`].
///
/// This virtual function table enables dynamic dispatch to a [`Poller`]`<`[`TcpListenerSocket`]`>`
/// instance.
fn listener_vtable() -> &'static RawTcpListenerVtable {
    &RawTcpListenerVtable {
        as_raw_sock: listener_as_raw_sock,
        poll_accept: listener_poll_accept,
        drop: listener_drop,
    }
}

/// Creates a new [`TcpListener`] bound to the specified socket address.
///
/// If the port number of the socket address is omitted or `0`, the operating system will assign
/// a port to this listener. The allocated port can be discovered by calling the
/// [`TcpSocketExt::local_addr()`] method.
///
/// [`TcpSocketExt::local_addr()`]: crate::net::TcpSocketExt::local_addr
pub(crate) fn bind_polled_listener(addr: SocketAddr) -> Result<TcpListener, Errno> {
    let listener = Poller::new(TcpListenerSocket::bind(addr)?);

    // SAFETY: The `vtable` matches the `data` pointer type.
    Ok(TcpListener::new(unsafe {
        RawTcpListener::new(Poller::into_raw(listener).cast(), listener_vtable())
    }))
}

/// Returns the [`RawTcpStream`]'s raw `spdk_sock` pointer.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
fn stream_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    this.polled().as_raw_sock()
}

/// Polls the connection state of the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns `Poll::Ready(Ok())` if the stream is connected, `Poll::Pending` if the
/// outgoing connection is pending, and `Poll::Ready(Err(`[`Errno`]`))` if the connection failed.
fn stream_poll_connected(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_connected(cx)
}

/// Attempts to read from the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
/// available and `Poll::Pending` if no data was available.
///
/// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
fn stream_poll_read(
    data: *const (),
    cx: &mut Context<'_>,
    buf: &mut [u8],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_read(cx, buf)
}

/// Attempts to read from the [`RawTcpStream`] into multiple buffers.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns number of bytes read in `Poll::Ready(Ok(num_bytes_read))` if data was
/// available and `Poll::Pending` if no data was available.
///
/// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
fn stream_poll_read_vectored(
    data: *const (),
    cx: &mut Context<'_>,
    bufs: &mut [std::io::IoSliceMut<'_>],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_read_vectored(cx, bufs)
}

/// Attempts to write to the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data was
/// written and `Poll::Pending` data cannot currently be written.
///
/// If the connection has failed, this method returns `Poll::Ready(Err(`[`Errno`]`))`.
fn stream_poll_write(
    data: *const (),
    cx: &mut Context<'_>,
    buf: &[u8],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_write(cx, buf)
}

/// Attempts to write to the [`RawTcpStream`] from multiple buffers.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns number of bytes written in `Poll::Ready(Ok(num_bytes_written))` if data was
/// written and `Poll::Pending` data cannot currently be written.
///
/// If the connection has failed, this method returns `Poll::Ready(Err(`[``Errno`]`))`.
fn stream_poll_write_vectored(
    data: *const (),
    cx: &mut Context<'_>,
    bufs: &[std::io::IoSlice<'_>],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_write_vectored(cx, bufs)
}

/// Attempts to flush buffered data from the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// The SPDK does not expose a means explicitly flush the buffer data in the socket, so this method
/// always returns `Poll::Ready(Ok())`.
fn stream_poll_flush(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_flush(cx)
}

/// Attempts to close the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns `Poll::Ready(Ok())` if the socket was successfully closed, and
/// `Poll::Ready(Err(`[`Errno`]`))` otherwise.
fn stream_poll_close(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_close(cx)
}

/// Drops the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>` instance.
fn stream_drop(data: *const ()) {
    drop(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });
}

/// Returns the [`RawTcpStreamVtable`] used by a [`RawTcpStream`].
///
/// This virtual function table enables dynamic dispatch to a [`Poller`]`<`[`TcpStreamSocket`]`>`
/// instance.
fn stream_vtable() -> &'static RawTcpStreamVtable {
    &RawTcpStreamVtable {
        as_raw_sock: stream_as_raw_sock,
        poll_connected: stream_poll_connected,
        poll_read: stream_poll_read,
        poll_read_vectored: stream_poll_read_vectored,
        poll_write: stream_poll_write,
        poll_write_vectored: stream_poll_write_vectored,
        poll_flush: stream_poll_flush,
        poll_close: stream_poll_close,
        drop: stream_drop,
    }
}

/// Accepts a new incoming connection producing a [`TcpStream`].
pub(crate) fn accept_polled_stream(accepted: Accepted) -> TcpStream {
    let stream = Poller::new(accepted.into_socket());

    // SAFETY: The `vtable` matches the `data` pointer type.
    TcpStream::new(unsafe { RawTcpStream::new(Poller::into_raw(stream).cast(), stream_vtable()) })
}

/// Creates a [`TcpStream`] connected to the specified socket address.
pub(crate) async fn connect_polled_stream(
    addr: SocketAddr,
    opts: &spdk_sock_opts,
) -> Result<TcpStream, Errno> {
    let stream = Poller::new_in_place(|stream| {
        TcpStreamSocket::connect_in_place(stream, addr, opts);
    });

    // SAFETY: The `vtable` matches the `data` pointer type.
    Connector::new(unsafe { RawTcpStream::new(Poller::into_raw(stream).cast(), stream_vtable()) })
        .await
}
