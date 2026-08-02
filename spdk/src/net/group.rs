//! Contains the SPDK socket group based TCP listener and stream implementations.
//!
//! An SPDK socket group provides a more efficient polling mechanism for multiple sockets than
//! creating separate pollers for each.
use std::{
    mem::{MaybeUninit, transmute},
    os::raw::c_void,
    pin::Pin,
    ptr::null_mut,
    rc::Rc,
    task::{Context, Poll},
};

use futures::task::noop_waker_ref;
use spdk_sys::{
    spdk_sock, spdk_sock_group, spdk_sock_group_add_sock, spdk_sock_group_close,
    spdk_sock_group_create, spdk_sock_group_poll, spdk_sock_group_remove_sock, spdk_sock_opts,
};

use crate::{
    errors::{EINVAL, Errno},
    net::ToSocketAddrs,
    task::{Polled, Poller},
    thread::Thread,
    to_result,
};

use super::{
    Accepted, AsRawSock, Connector, RawTcpListener, RawTcpListenerVtable, RawTcpStream,
    RawTcpStreamVtable, SocketAddr, TcpListener, TcpListenerSocket, TcpStream, TcpStreamSocket,
};

/// Handles events from a socket group.
pub(crate) trait SocketGroupEvent: AsRawSock {
    /// Handles an event notification from a socket group.
    fn handle_event(self: Pin<&mut Self>);
}

/// A stub trait implementation to enable initialization of a socket group event handler in-place.
impl<T> SocketGroupEvent for MaybeUninit<T>
where
    T: SocketGroupEvent,
{
    fn handle_event(self: Pin<&mut Self>) {
        unreachable!("handle_event called on uninitialized data");
    }
}

/// A member of a socket group.
struct Grouped<T>
where
    T: SocketGroupEvent,
{
    group: Rc<Poller<SocketGroupInner>>,
    sock: T,
}

impl<T> Grouped<T>
where
    T: SocketGroupEvent + Unpin,
{
    /// Creates a new instance of a socket group member.
    fn new(group: Rc<Poller<SocketGroupInner>>, sock: T) -> Box<Self> {
        Box::new(Self { group, sock })
    }
}

impl<T> Grouped<T>
where
    T: SocketGroupEvent,
{
    /// Initializes a new socket group member in-place.
    fn new_in_place<I>(group: Rc<Poller<SocketGroupInner>>, init_fn: I) -> Box<Self>
    where
        I: FnOnce(Pin<&mut MaybeUninit<T>>),
    {
        let mut this = Box::new_uninit();
        let this_ref = this.write(Grouped {
            group,
            sock: MaybeUninit::uninit(),
        });

        init_fn(unsafe { Pin::new_unchecked(&mut this_ref.sock) });

        // SAFETY: The group member has just been initialized.
        unsafe { transmute(this) }
    }

    /// Handles an event notification from the socket group.
    unsafe extern "C" fn handle_event(
        arg: *mut c_void,
        _group: *mut spdk_sock_group,
        _sock: *mut spdk_sock,
    ) {
        let grouped = unsafe { &mut *(arg as *mut Grouped<T>) };

        unsafe { Pin::new_unchecked(&mut grouped.sock) }.handle_event();
    }
}

impl<T> Drop for Grouped<T>
where
    T: SocketGroupEvent,
{
    fn drop(&mut self) {
        self.group
            .polled()
            .remove(&self.sock)
            .expect("socket removed");
    }
}

/// Returns the [`RawTcpListener`]'s raw `spdk_sock` pointer.
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpListenerSocket`]`>` instance.
fn listener_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this = unsafe { &mut *(data as *mut Grouped<TcpListenerSocket>) };

    this.sock.as_raw_sock()
}

/// Polls the [`RawTcpListener`] for an incoming connection.
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpListenerSocket`]`>` instance.
///
/// # Returns
///
/// If an incoming connection is available, this function returns `Poll<Ok(`[`Accepted`]`)>`. See
/// the [`TcpListener::accept`] for details on converting the `Accepted` instance into a
/// [`TcpStream`].
fn listener_poll_accept(data: *const (), cx: &mut Context<'_>) -> Poll<Result<Accepted, Errno>> {
    let this = unsafe { &mut *(data as *mut Grouped<TcpListenerSocket>) };

    Pin::new(&mut this.sock).poll_accept(cx)
}

/// Drops the [`RawTcpListener`].
///
/// This function is invoked through the [`RawTcpListenerVtable`] created by the [`listener_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpListenerSocket`]`>` instance.
fn listener_drop(data: *const ()) {
    drop(unsafe { Box::from_raw(data as *mut Grouped<TcpListenerSocket>) });
}

/// Returns the [`RawTcpListenerVtable`] used by a [`RawTcpListener`].
///
/// This virtual function table enables dynamic dispatch to a [`Grouped`]`<`[`TcpListenerSocket`]`>`
/// instance.
fn listener_vtable() -> &'static RawTcpListenerVtable {
    &RawTcpListenerVtable {
        as_raw_sock: listener_as_raw_sock,
        poll_accept: listener_poll_accept,
        drop: listener_drop,
    }
}

/// Returns the [`RawTcpStream`]'s raw `spdk_sock` pointer.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
fn stream_as_raw_sock(data: *const ()) -> *mut spdk_sock {
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    this.sock.as_raw_sock()
}

/// Polls the connection state of the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns `Poll::Ready(Ok())` if the stream is connected, `Poll::Pending` if the
/// outgoing connection is pending, and `Poll::Ready(Err(`[`Errno`]`))` if the connection failed.
fn stream_poll_connected(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_connected(cx)
}

/// Attempts to read from the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
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
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_read(cx, buf)
}

/// Attempts to read from the [`RawTcpStream`] into multiple buffers.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
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
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_read_vectored(cx, bufs)
}

/// Attempts to write to the [`RawTcpStream`] from multiple buffers.
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
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
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_write_vectored(cx, bufs)
}

/// Attempts to write to the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
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
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    // A socket group does not provide notification of write buffer availability, so we pass a no-op
    // waker to the socket's `poll_write` method and queue a thread message to the current thread to
    // poll for available write buffer space later.
    match Pin::new(&mut this.sock).poll_write(&mut Context::from_waker(noop_waker_ref()), buf) {
        Poll::Ready(res) => Poll::Ready(res),
        Poll::Pending => {
            let waker = cx.waker().clone();

            Thread::current().send_msg(move || {
                waker.wake();
            })?;
            Poll::Pending
        }
    }
}

/// Attempts to flush buffered data from the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// The SPDK does not expose a means explicitly flush the buffer data in the socket, so this method
/// always returns `Poll::Ready(Ok())`.
fn stream_poll_flush(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_flush(cx)
}

/// Attempts to close the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
///
/// # Returns
///
/// This method returns `Poll::Ready(Ok())` if the socket was successfully closed, and
/// `Poll::Ready(Err(`[`Errno`]`))` otherwise.
fn stream_poll_close(data: *const (), cx: &mut Context<'_>) -> Poll<Result<(), Errno>> {
    let this = unsafe { &mut *(data as *mut Grouped<TcpStreamSocket>) };

    Pin::new(&mut this.sock).poll_close(cx)
}

/// Drops the [`RawTcpStream`].
///
/// This function is invoked through the [`RawTcpStreamVtable`] crated by the [`stream_vtable`]
/// function. It enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>` instance.
fn stream_drop(data: *const ()) {
    drop(unsafe { Box::from_raw(data as *mut Grouped<TcpStreamSocket>) });
}

/// Returns the [`RawTcpStreamVtable`] used by a [`RawTcpStream`].
///
/// This virtual function table enables dynamic dispatch to a [`Grouped`]`<`[`TcpStreamSocket`]`>`
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

/// The shared state of a [`SocketGroup`] instance.
#[derive(Debug)]
struct SocketGroupInner {
    group: *mut spdk_sock_group,
}

impl SocketGroupInner {
    /// Creates a new [`SocketGroupInner`] instance, initializing it in-place.
    fn new_in_place(mut this: Pin<&mut MaybeUninit<Self>>) {
        let this = this.write(Self { group: null_mut() });

        this.group = unsafe { spdk_sock_group_create(this as *mut _ as *mut _) };
    }

    /// Creates a new [`TcpListener`] bound to the specified socket address and attached to this
    /// [`SocketGroup`].
    ///
    /// If the port number of the socket address is omitted or `0`, the operating system will assign
    /// a port to this listener. The allocated port can be discovered by calling the
    /// [`TcpSocketExt::local_addr()`] method.
    ///
    /// [`TcpSocketExt::local_addr()`]: crate::net::TcpSocketExt::local_addr
    fn bind(this: &Rc<Poller<Self>>, addr: SocketAddr) -> Result<TcpListener, Errno> {
        let mut listener = Grouped::new(this.clone(), TcpListenerSocket::bind(addr)?);

        to_result!(unsafe {
            spdk_sock_group_add_sock(
                this.polled().group,
                listener.sock.as_raw_sock(),
                Some(Grouped::<TcpListenerSocket>::handle_event),
                listener.as_mut() as *mut _ as *mut _,
            )
        })?;

        // SAFETY: The `vtable` matches the `data` pointer type.
        Ok(TcpListener::new(unsafe {
            RawTcpListener::new(Box::into_raw(listener).cast(), listener_vtable())
        }))
    }

    /// Adds a new incoming connection producing a [`TcpStream`] attached to this [`SocketGroup`].
    fn add(this: &Rc<Poller<Self>>, accepted: Accepted) -> Result<TcpStream, Errno> {
        let mut stream = Grouped::new(this.clone(), accepted.into_socket());

        to_result!(unsafe {
            spdk_sock_group_add_sock(
                this.polled().group,
                stream.sock.as_raw_sock(),
                Some(Grouped::<TcpStreamSocket>::handle_event),
                stream.as_mut() as *mut _ as *mut _,
            )
        })?;

        // SAFETY: The `vtable` matches the `data` pointer type.
        Ok(TcpStream::new(unsafe {
            RawTcpStream::new(Box::into_raw(stream).cast(), stream_vtable())
        }))
    }

    /// Creates a [`TcpStream`] connected to the specified socket address and attached to this
    /// [`SocketGroup`].
    async fn connect(
        this: &Rc<Poller<Self>>,
        addr: SocketAddr,
        opts: &spdk_sock_opts,
    ) -> Result<TcpStream, Errno> {
        let mut stream = Grouped::new_in_place(this.clone(), |stream| {
            TcpStreamSocket::connect_in_place(stream, addr, opts)
        });

        to_result!(unsafe {
            spdk_sock_group_add_sock(
                this.polled().group,
                stream.sock.as_raw_sock(),
                Some(Grouped::<TcpStreamSocket>::handle_event),
                stream.as_mut() as *mut _ as *mut _,
            )
        })?;

        // SAFETY: The `vtable` matches the `data` pointer type.
        Connector::new(unsafe { RawTcpStream::new(Box::into_raw(stream).cast(), stream_vtable()) })
            .await
    }

    /// Removes a TCP socket from this group.
    ///
    /// This method is called from the [`Grouped<T: AsRawSock>::drop()`] method when a
    /// [`TcpListener`] or [`TcpStream`] (via [`RawTcpListener`] or [`RawTcpStream`], respectively)
    /// group member is dropped. It is not necessary to manually call this method.
    fn remove<T>(&self, sock: &T) -> Result<(), Errno>
    where
        T: AsRawSock,
    {
        to_result!(unsafe { spdk_sock_group_remove_sock(self.group, sock.as_raw_sock()) })
    }
}

impl Drop for SocketGroupInner {
    fn drop(&mut self) {
        to_result!(unsafe { spdk_sock_group_close(&mut self.group) }).expect("socket group closed");
    }
}

impl Polled for SocketGroupInner {
    fn poll(self: Pin<&mut Self>) -> bool {
        unsafe { spdk_sock_group_poll(self.group) != 0 }
    }
}

/// A group of TCP sockets on a single [`Thread`].
///
/// `SocketGroup` provides a more efficient polling mechanism for multiple TCP sockets than creating
/// separate pollers for each.
pub struct SocketGroup(Rc<Poller<SocketGroupInner>>);

impl SocketGroup {
    /// Creates a new [`SocketGroup`].
    pub fn new() -> Self {
        Self(Rc::new(Poller::new_in_place(
            SocketGroupInner::new_in_place,
        )))
    }

    /// Creates a new [`TcpListener`] bound to the specified socket address and attached to this
    /// group.
    ///
    /// If the port number of the socket address is omitted or `0`, the operating system will assign
    /// a port to this listener. The allocated port can be discovered by calling the
    /// [`TcpSocketExt::local_addr()`] method.
    ///
    /// If `addr` yields multiple socket address, `bind` will attempt listen on each until one
    /// succeeds and returns a listener. If no address can be successfully bound, `Err(EINVAL)` is
    /// returned.
    ///
    /// [`TcpSocketExt::local_addr()`]: crate::net::TcpSocketExt::local_addr
    pub async fn bind<A: ToSocketAddrs>(&self, addrs: A) -> Result<TcpListener, Errno> {
        for addr in addrs.to_socket_addr().await? {
            match SocketGroupInner::bind(&self.0, addr) {
                Ok(listener) => return Ok(listener),
                Err(_) => continue,
            }
        }

        Err(EINVAL)
    }

    /// Creates a TCP connection to the specified socket address, returning a [`TcpStream`] attached
    /// to this group on success.
    ///
    /// If `addr` yields multiple socket addresses, `connect` will attempt to connect to each until
    /// one succeeds and returns a stream. If no address can be successfully connected, the error
    /// from the last connection attempt is returned.
    pub async fn connect(
        &self,
        addr: SocketAddr,
        opts: &spdk_sock_opts,
    ) -> Result<TcpStream, Errno> {
        SocketGroupInner::connect(&self.0, addr, opts).await
    }

    /// Adds a new incoming connection producing a [`TcpStream`] attached to this group.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use spdk::net::{SocketGroup, TcpListener, TcpStream};
    ///
    /// let group = SocketGroup::new();
    /// let listener = group.bind("127.0.0.1:8080").await?;
    /// let remote = group.add(listener.accept().await?)?;
    /// ```
    pub fn add(&self, accepted: Accepted) -> Result<TcpStream, Errno> {
        SocketGroupInner::add(&self.0, accepted)
    }
}

impl Clone for SocketGroup {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Default for SocketGroup {
    fn default() -> Self {
        Self::new()
    }
}
