use std::{
    ffi::{
        CStr,
        CString,
    },
    fmt::Display,
    future::{
        Future,

        ready,
    },
    mem::{self},
    option::{self},
    ptr::{
        self,

        addr_of,
    },
    str::FromStr,
    sync::Weak,
    task::Poll,
};

use errno::Errno;
use lazy_regex::{regex_captures, regex_is_match};
use libc::{
    addrinfo,

    AF_INET,
    AF_INET6,
    NI_NUMERICHOST,
    NI_NUMERICSERV,

    getnameinfo,
};

use crate::{
    eai_to_result,
    errors::EINVAL,
    task::{
        Polled,
        Poller,
        Promise,
        Promissory,
    },
};

use super::libanl::{
    gaicb,

    EAI_INPROGRESS,
    GAI_NOWAIT,

    gai_error,
    getaddrinfo_a,
};

macro_rules! impl_socket_addr {
    ($i:ident) => {
        #[derive(Clone, Debug)]
        pub struct $i {
            host: CString,
            port: u16,
        }

        impl $i {
            pub fn new(host: CString, port: u16) -> Self {
                Self {
                    host,
                    port,
                }
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

impl_socket_addr!(SocketAddrV4);

impl std::fmt::Display for SocketAddrV4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host().to_string_lossy(), self.port())
    }
}

impl_socket_addr!(SocketAddrV6);

impl std::fmt::Display for SocketAddrV6 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]:{}", self.host().to_string_lossy(), self.port())
    }
}


#[derive(Clone, Debug)]
pub enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

impl SocketAddr {
    pub fn new(host: CString, port: u16) -> Self {
        if regex_is_match!(r#"\d+.\d+\d+.\d+"#, &host.to_string_lossy()) {
            Self::V4(SocketAddrV4::new(host, port))
        } else {
            Self::V6(SocketAddrV6::new(host, port))
        }
    }

    pub fn ip(&self) -> &CStr {
        match self {
            Self::V4(addr) => addr.host(),
            Self::V6(addr) => addr.host(),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            Self::V4(addr) => addr.port(),
            Self::V6(addr) => addr.port(),
        }
    }

    pub fn is_iv4(&self) -> bool {
        matches!(self, Self::V4(_))
    }

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

    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        Ok(std::net::SocketAddr::from_str(_s).map_err(|_| EINVAL)?.into())
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

impl From<*mut addrinfo> for SocketAddr {
    fn from(addr: *mut addrinfo) -> Self {
        let mut host = [0u8; 1025];
        let mut serv = [0u8; 32];

        eai_to_result!(unsafe {
            getnameinfo(
                (*addr).ai_addr,
                (*addr).ai_addrlen,
                host.as_mut_ptr().cast(),
                1025,
                serv.as_mut_ptr().cast(),
                32,
                NI_NUMERICHOST | NI_NUMERICSERV,
            )
        })
        .expect("valid IP address");

        let host = CStr::from_bytes_until_nul(&host)
            .expect("valid IP address")
            .into();
        let serv = CStr::from_bytes_until_nul(&serv)
            .expect("valid port")
            .to_string_lossy()
            .parse()
            .expect("valid numeric port");

        match unsafe { (*addr).ai_family } {
            AF_INET => SocketAddr::V4(SocketAddrV4::new(host, serv)),
            AF_INET6 => SocketAddr::V6(SocketAddrV6::new(host, serv)),
            _ => unreachable!(),
        }
    }
}

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

        let addr = self.current.into();
        self.current = unsafe { (*self.current).ai_next };

        Some(addr)
    }
}

struct LookupHost {
    host: CString,
    service: Option<CString>,
    prom: Weak<Promissory<SocketAddrIter, Poller<Self>>>,
    req: gaicb,
    hint: addrinfo,
}

impl LookupHost {
    fn new(host: &str, service: Option<&str>, prom: Weak<Promissory<SocketAddrIter, Poller<Self>>>) -> Poller<Self> {
        let host = CString::new(host).expect("valid host");
        let service = service.map(|s| CString::new(s).expect("valid service"));
        let mut this = Poller::new(Self {
            host,
            service,
            req: unsafe {mem::zeroed() },
            hint: unsafe { mem::zeroed() },
            prom,
        });

        let lh = this.polled_mut();

        lh.req.ar_name = lh.host.as_ptr();
        lh.req.ar_service = lh.service.as_ref().map_or(ptr::null(), |s| s.as_ptr());

        // Limit the socket type because SPDK only supports stream sockets currently.
        lh.hint.ai_flags = libc::AI_V4MAPPED | libc::AI_ADDRCONFIG;
        lh.hint.ai_socktype = libc::SOCK_STREAM;
        lh.req.ar_request = addr_of!(lh.hint);

        this
    }

    fn start(&self) -> Poll<Result<SocketAddrIter, Errno>> {
        let list = [addr_of!(self.req)].as_ptr();

        eai_to_result!(unsafe { getaddrinfo_a(GAI_NOWAIT, list as *mut _, 1, ptr::null_mut()) })
            .err()
            .map_or(Poll::Pending, |e| Poll::Ready(Err(e.into())))
    }

    fn poll_request(&self) -> Poll<Result<SocketAddrIter, Errno>> {
        let res = eai_to_result!(unsafe { gai_error(&self.req as *const _) });

        match res {
            Ok(()) => {
                Poll::Ready(Ok(SocketAddrIter::new(self.req.ar_result)))
            },
            Err(EAI_INPROGRESS) => Poll::Pending,
            Err(e) => Poll::Ready(Err(e.into())),
        }
    }
}

impl Polled for LookupHost {
    fn poll(&mut self) -> bool {
        if let Some(prom) = self.prom.upgrade() {
            if let Poll::Ready(res) = self.poll_request() {
                Promissory::set_result(prom, res);

                return true;
            }
        }

        false
    }
}

pub async fn lookup_host<A>(addr: A) -> Result<A::Iter, Errno>
where
    A: ToSocketAddrs,
{
    addr.to_socket_addr().await
}

/// A trait for objects that can be converted or resolved to one or more
/// [`SocketAddr`] objects.
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
    type Iter: Iterator<Item=SocketAddr>;

    /// Converts or resolves this object into an iterator of [`SocketAddr`]
    /// objects.
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
        Promise::with_context_cyclic(
            |w| LookupHost::new(&self.0, self.1, w.clone()),
            |p| Promissory::user_context(p).polled().start(),
        )
    }
}

impl ToSocketAddrs for (&str, &str) {
    type Iter = SocketAddrIter;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        Promise::with_context_cyclic(
            |w| LookupHost::new(&self.0, Some(&self.1), w.clone()),
            |p| Promissory::user_context(p).polled().start(),
        )
    }
}

impl ToSocketAddrs for str {
    type Iter = SocketAddrIter;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        async {
            if let Some((_, host, service)) = regex_captures!(r#"^\[?([^\]]*)\]?:(\d+)$"#, self) {
                return Promise::with_context_cyclic(
                    |w| LookupHost::new(host, Some(service), w.clone()),
                    |p| Promissory::user_context(p).polled().start(),
                ).await;
            }

            Err(EINVAL)
        }
    }
}

impl ToSocketAddrs for String {
    type Iter = SocketAddrIter;

    fn to_socket_addr(&self) -> impl Future<Output = Result<Self::Iter, Errno>> {
        (&**self).to_socket_addr()
    }
}
