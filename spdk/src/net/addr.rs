use std::{
    ffi::{CStr, CString},
    fmt::Display,
    future::{ready, Future},
    marker::PhantomPinned,
    mem::{self, replace, MaybeUninit},
    option::{self},
    pin::Pin,
    ptr::{self, addr_of},
    str::FromStr,
    task::{Context, Poll, Waker},
};

use lazy_regex::{regex_captures, regex_is_match};
use libc::{addrinfo, getnameinfo, AF_INET, AF_INET6, NI_NUMERICHOST, NI_NUMERICSERV};
use ternary_rs::if_else;

use crate::{
    eai_to_result,
    errors::{Errno, EINVAL},
    net::libanl::{self, getaddrinfo_a},
    task::{Polled, Poller},
};

use super::libanl::{gai_error, gaicb, EAI_INPROGRESS, GAI_NOWAIT};

macro_rules! impl_socket_addr {
    ($i:ident, $($d:expr),+ $(,)?) => {
        #[derive(Clone, Debug)]
        #[doc = concat!($($d, "\n"),+)]
        pub struct $i {
            host: CString,
            port: u16,
        }

        impl $i {
            pub fn new(host: CString, port: u16) -> Self {
                Self { host, port }
            }

            pub fn host(&self) -> &CStr {
                &self.host
            }

            pub fn port(&self) -> u16 {
                self.port
            }
        }
    };
}

impl_socket_addr! {
    SocketAddrV4,
    "An IPv4 socket address.",
    "",
    "An IPv4 socket address consists of an IPv4 address as a string in dotted-decimal format and ",
    "16-bit port number.",
    "",
    "See [`SocketAddr`] for a type that can represent either IPv4 or IPv6 addresses."
}

impl std::fmt::Display for SocketAddrV4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host().to_string_lossy(), self.port())
    }
}

impl_socket_addr! {
    SocketAddrV6,
    "",
    "An IPv6 socket address consists of an IPv6 address as a string in colon-delimited hexadecimal ",
    "format and 16-bit port number.",
    "",
    "See [`SocketAddr`] for a type that can represent either IPv4 or IPv6 addresses."
}

impl std::fmt::Display for SocketAddrV6 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]:{}", self.host().to_string_lossy(), self.port())
    }
}

/// An Internet socket address, either IPv4 or IPv6.
#[derive(Clone, Debug)]
pub enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

impl SocketAddr {
    /// Creates a new `SocketAddr` from a string containing an IPv4 or IPv6 address.
    pub fn new(host: CString, port: u16) -> Self {
        if regex_is_match!(r#"\d+.\d+\d+.\d+"#, &host.to_string_lossy()) {
            Self::V4(SocketAddrV4::new(host, port))
        } else {
            Self::V6(SocketAddrV6::new(host, port))
        }
    }

    /// Creates a new `SocketAddr` from an [`addrinfo`] structure.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `addr` points to a valid `addrinfo` structure.
    ///
    /// [`addrinfo`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo.3.html
    unsafe fn from_raw(addr: *mut addrinfo) -> Self {
        let mut host = [0u8; 1025];
        let mut serv = [0u8; 32];

        eai_to_result!(getnameinfo(
            (*addr).ai_addr,
            (*addr).ai_addrlen,
            host.as_mut_ptr().cast(),
            1025,
            serv.as_mut_ptr().cast(),
            32,
            NI_NUMERICHOST | NI_NUMERICSERV,
        ))
        .expect("valid IP address");

        let host = CStr::from_bytes_until_nul(&host)
            .expect("valid IP address")
            .into();
        let serv = CStr::from_bytes_until_nul(&serv)
            .expect("valid port")
            .to_string_lossy()
            .parse()
            .expect("valid numeric port");

        match (*addr).ai_family {
            AF_INET => SocketAddr::V4(SocketAddrV4::new(host, serv)),
            AF_INET6 => SocketAddr::V6(SocketAddrV6::new(host, serv)),
            _ => unreachable!(),
        }
    }

    /// Returns the IP address associated with this socket address.
    pub fn ip(&self) -> &CStr {
        match self {
            Self::V4(addr) => addr.host(),
            Self::V6(addr) => addr.host(),
        }
    }

    /// Returns the port asscciated with this socket address.
    pub fn port(&self) -> u16 {
        match self {
            Self::V4(addr) => addr.port(),
            Self::V6(addr) => addr.port(),
        }
    }

    /// Returns whether this socket address contains an IPv4 address.
    pub fn is_iv4(&self) -> bool {
        matches!(self, Self::V4(_))
    }

    /// Returns whether this socket address contains an IPv6 address.
    pub fn is_iv6(&self) -> bool {
        matches!(self, Self::V6(_))
    }
}

impl Display for SocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V4(addr) => write!(f, "{}", addr),
            Self::V6(addr) => write!(f, "{}", addr),
        }
    }
}

impl FromStr for SocketAddr {
    type Err = Errno;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(std::net::SocketAddr::from_str(s)
            .map_err(|_| EINVAL)?
            .into())
    }
}

impl From<std::net::SocketAddr> for SocketAddr {
    fn from(std_addr: std::net::SocketAddr) -> Self {
        let host = CString::new(std_addr.ip().to_string()).expect("valid IP address");
        let port = std_addr.port();

        match std_addr {
            std::net::SocketAddr::V4(_) => Self::V4(SocketAddrV4::new(host, port)),
            std::net::SocketAddr::V6(_) => Self::V6(SocketAddrV6::new(host, port)),
        }
    }
}

impl TryFrom<&str> for SocketAddr {
    type Error = Errno;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        SocketAddr::from_str(s)
    }
}

/// An iterator over the results returned by the [`ToSocketAddrs`] trait and [`resolve`] function.
#[derive(Debug)]
pub struct SocketAddrIter {
    addrinfo: *mut addrinfo,
    current: *mut addrinfo,
}

impl SocketAddrIter {
    fn new(addrinfo: *mut addrinfo) -> Self {
        Self {
            addrinfo,
            current: addrinfo,
        }
    }
}

impl Drop for SocketAddrIter {
    fn drop(&mut self) {
        if !self.addrinfo.is_null() {
            unsafe { libc::freeaddrinfo(self.addrinfo) };
        }
    }
}

impl Iterator for SocketAddrIter {
    type Item = SocketAddr;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            return None;
        }

        let addr = unsafe { SocketAddr::from_raw(self.current) };
        self.current = unsafe { (*self.current).ai_next };

        Some(addr)
    }
}

/// The state of an asynchronous socket address lookup operation.
#[derive(Debug)]
enum ResolverState {
    /// The resolver has been newly created and no asynchronous socket address lookup has been
    /// started.
    Idle,

    /// An asynchronous socket address lookup is in-progress with an optional [`Waker`] to receive
    /// completion notification.
    Resolving(Waker),

    /// The lookup operation has completed and the pending waker is being awakened.
    Waking,

    /// The asynchronous socket address lookup is complete.
    Complete,
}

impl ResolverState {
    /// Polls the state of the resolver, advancing to the next state if possible.
    fn poll(&mut self, cx: &Context<'_>) -> Self {
        match self {
            Self::Idle | Self::Resolving(_) | Self::Waking => {
                replace(self, Self::Resolving(cx.waker().clone()))
            }
            _ => unreachable!("resolver polled in unexpected state: {:?}", self),
        }
    }

    /// Sets the state to [`Self::Waking`] and returns the [`Waker`] to wake.
    fn set_waking(&mut self) -> Waker {
        let prev_state = replace(self, Self::Waking);

        if let Self::Resolving(waker) = prev_state {
            return waker;
        }

        unreachable!("set_waking called in unexpected state: {:?}", prev_state);
    }

    /// Sets the state to [`Self::Complete`].
    fn set_complete(&mut self) {
        *self = Self::Complete;
    }
}

/// Manages the internal state of an asynchronous socket address lookup operation, polling for completion.
///
/// # Safety
///
/// This type contains pointers to memory within itself and must remain pinned after initialization.
struct ResolverInner {
    host: CString,
    service: Option<CString>,
    state: ResolverState,
    req: gaicb,
    hint: addrinfo,
    _pinned: PhantomPinned,
}

impl ResolverInner {
    /// Initializes a new `ResolverInner` in-place at the memory referenced by `this`.
    fn new_in_place(mut this: Pin<&mut MaybeUninit<Self>>, host: &str, service: Option<&str>) {
        let host = CString::new(host).expect("valid host");
        let service = service.map(|s| CString::new(s).expect("valid service"));

        let this = unsafe { Pin::get_unchecked_mut(this.as_mut()) }.write(Self {
            host,
            service,
            state: ResolverState::Idle,
            req: unsafe { mem::zeroed() },
            hint: unsafe { mem::zeroed() },
            _pinned: PhantomPinned,
        });

        this.req.ar_name = this.host.as_ptr();
        this.req.ar_service = this.service.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        // Limit the socket type because SPDK only supports stream sockets currently.
        this.hint.ai_flags = libc::AI_V4MAPPED | libc::AI_ADDRCONFIG;
        this.hint.ai_socktype = libc::SOCK_STREAM;
        this.req.ar_request = addr_of!(this.hint);
    }

    /// Returns a mutable reference to the resolver state.
    #[inline]
    fn state_mut(self: Pin<&mut Self>) -> &mut ResolverState {
        // SAFETY: We are mapping to a field of a pinned value.
        unsafe { Pin::map_unchecked_mut(self, |s| &mut s.state) }.get_mut()
    }

    /// Called when polled in the [`Idle`] state to start the asynchronous socket address lookup.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(())` if the lookup operation was started successfully. Otherwise, it
    /// returns a `Err(`[`libanl::Error`]`)` result.
    ///
    /// [`Idle`]: ResolverState::Idle
    fn poll_start(self: Pin<&Self>) -> Result<(), libanl::Error> {
        let list = [addr_of!(self.req)].as_ptr();

        eai_to_result!(unsafe { getaddrinfo_a(GAI_NOWAIT, list as *mut _, 1, ptr::null_mut()) })
    }

    /// Called to poll the result of an asynchronous socket address lookup.
    ///
    /// # Returns
    ///
    /// This method returns `Ok(Some(`[`SocketAddrIter`]`))` if the lookup operation completed
    /// successfully, `Ok(None)` if the operation is still in progress and
    /// `Err(`[`libanl::Error`]`)` on error.
    fn poll_result(self: Pin<&Self>) -> Result<Option<SocketAddrIter>, libanl::Error> {
        eai_to_result!(unsafe { gai_error(&self.req as *const _) })
            .err()
            .map(|e| if_else!(e == EAI_INPROGRESS, Ok(None), Err(e)))
            .unwrap_or_else(|| Ok(Some(SocketAddrIter::new(self.req.ar_result))))
    }

    /// Whether the asynchronous socket address lookup is done.
    fn is_ready(self: Pin<&Self>) -> bool {
        !matches!(
            eai_to_result!(unsafe { gai_error(&self.req as *const _) }),
            Err(EAI_INPROGRESS)
        )
    }
}

impl Polled for ResolverInner {
    fn poll(self: Pin<&mut Self>) -> bool {
        if self.as_ref().is_ready() {
            self.state_mut().set_waking().wake();
            return true;
        }

        false
    }
}

impl Future for ResolverInner {
    type Output = Result<SocketAddrIter, Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.as_mut().state_mut().poll(cx);

        match state {
            ResolverState::Idle => self
                .as_ref()
                .poll_start()
                .err()
                .map_or(Poll::Pending, |e| Poll::Ready(Err(e.into()))),
            ResolverState::Resolving(_) => Poll::Pending,
            ResolverState::Waking => match self.as_ref().poll_result() {
                Ok(Some(res)) => {
                    self.state_mut().set_complete();

                    Poll::Ready(Ok(res))
                }
                Ok(None) => Poll::Pending,
                Err(e) => {
                    self.state_mut().set_complete();

                    Poll::Ready(Err(e.into()))
                }
            },
            _ => panic!("poll called in unexpected state: {:?}", state),
        }
    }
}

/// A [`Future`] for performing an asynchronous socket address lookup operation.
struct Resolver(Poller<ResolverInner>);

impl Resolver {
    fn new(host: &str, service: Option<&str>) -> Self {
        Self(Poller::new_in_place(move |inner| {
            ResolverInner::new_in_place(inner, host, service)
        }))
    }
}

impl Future for Resolver {
    type Output = Result<SocketAddrIter, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll(cx)
    }
}

/// Converts or resolves addresses to [`SocketAddr`] values asynchronously.
///
/// # Examples
///
/// ```no_run
/// use spdk::net::resolve;
///
/// let addr = resolve("localhost:8080").await?.next().unwrap();
/// println!("resolved {:?}", addr);
/// ```
pub async fn resolve<A>(addr: A) -> Result<A::Iter, Errno>
where
    A: ToSocketAddrs,
{
    addr.to_socket_addr().await
}

/// A trait for objects that can be converted or resolved to one or more [`SocketAddr`] objects.
///
/// This trait is an asynchronous version of [`std::net::ToSocketAddrs`].
///
/// # Examples
///
/// ```no_run
/// use spdk::net::ToSocketAddrs;
///
/// let addr = "localhost:8080".to_socket_addrs().await?.next().unwrap();
/// println!("resolved {:?}", addr);
/// ```
pub trait ToSocketAddrs {
    type Iter: Iterator<Item = SocketAddr>;

    /// Converts or resolves this object into an iterator of [`SocketAddr`] objects.
    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>>;
}

impl ToSocketAddrs for SocketAddr {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        ready(Ok(Some(self.clone()).into_iter()))
    }
}

impl ToSocketAddrs for SocketAddrV4 {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        ready(Ok(Some(SocketAddr::V4(self.clone())).into_iter()))
    }
}

impl ToSocketAddrs for SocketAddrV6 {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        ready(Ok(Some(SocketAddr::V6(self.clone())).into_iter()))
    }
}

impl ToSocketAddrs for (&str, Option<&str>) {
    type Iter = SocketAddrIter;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        Resolver::new(self.0, self.1)
    }
}

impl ToSocketAddrs for (&str, &str) {
    type Iter = SocketAddrIter;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        Resolver::new(self.0, Some(self.1))
    }
}

impl ToSocketAddrs for &str {
    type Iter = SocketAddrIter;

    async fn to_socket_addr(&self) -> Result<Self::Iter, Errno> {
        if let Some((_, host, service)) = regex_captures!(r#"^\[?([^\]]+)\]?:(.+)$"#, self) {
            return Resolver::new(host, Some(service)).await;
        }

        Err(EINVAL)
    }
}

impl ToSocketAddrs for String {
    type Iter = SocketAddrIter;

    async fn to_socket_addr(&self) -> Result<Self::Iter, Errno> {
        self.as_str().to_socket_addr().await
    }
}
