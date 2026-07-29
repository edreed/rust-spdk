use std::{
    mem::ManuallyDrop,
    pin::Pin,
    task::{Context, Poll},
};

use spdk_sys::{spdk_sock, spdk_sock_opts};

use crate::{errors::Errno, task::Poller};

use super::{
    Accepted, AsRawSock, RawTcpListener, RawTcpListenerVtable, RawTcpStream, RawTcpStreamVtable,
    SocketAddr, TcpListenerSocket, TcpStreamSocket,
};

fn listener_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });

    this.polled().as_raw_sock()
}

fn listener_poll_accept(data: *const (), cx: &mut Context<'_>) -> Poll<Result<Accepted, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });

    Pin::new(this.polled_mut()).poll_accept(cx)
}

fn listener_drop(data: *const ()) {
    drop(unsafe { Poller::from_raw(data as *const Poller<TcpListenerSocket>) });
}

fn listener_vtable() -> &'static RawTcpListenerVtable {
    &RawTcpListenerVtable {
        as_raw_sock: listener_as_raw_sock,
        poll_accept: listener_poll_accept,
        drop: listener_drop,
    }
}

pub(crate) fn bind_polled_listener(addr: SocketAddr) -> Result<RawTcpListener, Errno> {
    let listener = Poller::new(TcpListenerSocket::bind(addr)?);

    Ok(RawTcpListener::new(
        Poller::into_raw(listener).cast(),
        listener_vtable(),
    ))
}

fn stream_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    this.polled().as_raw_sock()
}

fn stream_poll_connected(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_connected(cx)
}

fn stream_poll_read(
    data: *const (),
    cx: &mut Context<'_>,
    buf: &mut [u8],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_read(cx, buf)
}

fn stream_poll_write(
    data: *const (),
    cx: &mut Context<'_>,
    buf: &[u8],
) -> Poll<Result<usize, Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_write(cx, buf)
}

fn stream_poll_flush(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_flush(cx)
}

fn stream_poll_close(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let mut this =
        ManuallyDrop::new(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });

    Pin::new(this.polled_mut()).poll_close(cx)
}

fn stream_drop(data: *const ()) {
    drop(unsafe { Poller::from_raw(data as *const Poller<TcpStreamSocket>) });
}

fn stream_vtable() -> &'static RawTcpStreamVtable {
    &RawTcpStreamVtable {
        as_raw_sock: stream_as_raw_sock,
        poll_connected: stream_poll_connected,
        poll_read: stream_poll_read,
        poll_write: stream_poll_write,
        poll_flush: stream_poll_flush,
        poll_close: stream_poll_close,
        drop: stream_drop,
    }
}

pub(crate) fn connect_polled_stream(addr: SocketAddr, opts: &spdk_sock_opts) -> RawTcpStream {
    let stream = Poller::new_in_place(|stream| {
        TcpStreamSocket::connect_in_place(stream, addr, opts);
    });

    RawTcpStream::new(Poller::into_raw(stream).cast(), stream_vtable())
}

pub(crate) fn new_polled_stream(sock: *mut spdk_sock) -> RawTcpStream {
    let stream = Poller::new(TcpStreamSocket::new(sock));

    RawTcpStream::new(Poller::into_raw(stream).cast(), stream_vtable())
}
