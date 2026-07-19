//! Common error definitions.
use std::{
    error::Error,
    ffi::CStr,
    fmt::{Debug, Display},
    io::ErrorKind,
};

use libc;
use spdk_macros::define_errno;

/// Returns the last error set by a system or library call.
pub fn errno() -> Errno {
    Errno(unsafe { *libc::__errno_location() })
}

/// A wrapper around a Linux error code.
#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct Errno(#[doc = "A linux error code."] i32);

impl Errno {
    /// Creates a new `Errno` from a Linux error code.
    pub const fn new(errno: i32) -> Self {
        Self(errno)
    }

    /// Returns the Linux error code wrapped by this instance.
    pub fn errno(&self) -> i32 {
        self.0
    }
}

impl Display for Errno {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = unsafe { &CStr::from_ptr(libc::strerror(self.0)).to_string_lossy() };

        f.write_str(msg)
    }
}

impl Debug for Errno {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Errno").field(&self.0).finish()
    }
}

impl Error for Errno {}

define_errno! {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENXIO = 6,
    E2BIG = 7,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    ENOTBLK = 15,
    EBUSY = 16,
    EEXIST = 17,
    EXDEV = 18,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    ENFILE = 23,
    EMFILE = 24,
    ENOTTY = 25,
    ETXTBSY = 26,
    EFBIG = 27,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EMLINK = 31,
    EPIPE = 32,
    EDOM = 33,
    ERANGE = 34,
    EDEADLK = 35,
    ENAMETOOLONG = 36,
    ENOLCK = 37,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    ENOMSG = 42,
    EIDRM = 43,
    ECHRNG = 44,
    EL2NSYNC = 45,
    EL3HLT = 46,
    EL3RST = 47,
    ELNRNG = 48,
    EUNATCH = 49,
    ENOCSI = 50,
    EL2HLT = 51,
    EBADE = 52,
    EBADR = 53,
    EXFULL = 54,
    ENOANO = 55,
    EBADRQC = 56,
    EBADSLT = 57,
    EBFONT = 59,
    ENOSTR = 60,
    ENODATA = 61,
    ETIME = 62,
    ENOSR = 63,
    ENONET = 64,
    ENOPKG = 65,
    EREMOTE = 66,
    ENOLINK = 67,
    EADV = 68,
    ESRMNT = 69,
    ECOMM = 70,
    EPROTO = 71,
    EMULTIHOP = 72,
    EDOTDOT = 73,
    EBADMSG = 74,
    EOVERFLOW = 75,
    ENOTUNIQ = 76,
    EBADFD = 77,
    EREMCHG = 78,
    ELIBACC = 79,
    ELIBBAD = 80,
    ELIBSCN = 81,
    ELIBMAX = 82,
    ELIBEXEC = 83,
    EILSEQ = 84,
    ERESTART = 85,
    ESTRPIPE = 86,
    EUSERS = 87,
    ENOTSOCK = 88,
    EDESTADDRREQ = 89,
    EMSGSIZE = 90,
    EPROTOTYPE = 91,
    ENOPROTOOPT = 92,
    EPROTONOSUPPORT = 93,
    ESOCKTNOSUPPORT = 94,
    EOPNOTSUPP = 95,
    EPFNOSUPPORT = 96,
    EAFNOSUPPORT = 97,
    EADDRINUSE = 98,
    EADDRNOTAVAIL = 99,
    ENETDOWN = 100,
    ENETUNREACH = 101,
    ENETRESET = 102,
    ECONNABORTED = 103,
    ECONNRESET = 104,
    ENOBUFS = 105,
    EISCONN = 106,
    ENOTCONN = 107,
    ESHUTDOWN = 108,
    ETOOMANYREFS = 109,
    ETIMEDOUT = 110,
    ECONNREFUSED = 111,
    EHOSTDOWN = 112,
    EHOSTUNREACH = 113,
    EALREADY = 114,
    EINPROGRESS = 115,
    ESTALE = 116,
    EDQUOT = 122,
    ENOMEDIUM = 123,
    EMEDIUMTYPE = 124,
    ECANCELED = 125,
    ENOKEY = 126,
    EKEYEXPIRED = 127,
    EKEYREVOKED = 128,
    EKEYREJECTED = 129,
    EOWNERDEAD = 130,
    ENOTRECOVERABLE = 131,
    EHWPOISON = 133,
    ERFKILL = 132,
}

define_errno! {
    EWOULDBLOCK = 11,
    ENOTSUP = 95,
}

impl From<i32> for Errno {
    fn from(i: i32) -> Self {
        Self(i)
    }
}

impl From<std::io::Error> for Errno {
    fn from(e: std::io::Error) -> Self {
        if let Some(e) = e.raw_os_error() {
            return Errno(e);
        }

        e.kind().into()
    }
}

impl From<ErrorKind> for Errno {
    fn from(value: ErrorKind) -> Self {
        match value {
            ErrorKind::NotFound => ENOENT,
            ErrorKind::PermissionDenied => EPERM,
            ErrorKind::ConnectionRefused => ECONNREFUSED,
            ErrorKind::ConnectionReset => ECONNRESET,
            ErrorKind::HostUnreachable => EHOSTUNREACH,
            ErrorKind::NetworkUnreachable => ENETUNREACH,
            ErrorKind::ConnectionAborted => ECONNABORTED,
            ErrorKind::NotConnected => ENOTCONN,
            ErrorKind::AddrInUse => EADDRINUSE,
            ErrorKind::AddrNotAvailable => EADDRNOTAVAIL,
            ErrorKind::NetworkDown => ENETDOWN,
            ErrorKind::BrokenPipe => EPIPE,
            ErrorKind::AlreadyExists => EEXIST,
            ErrorKind::WouldBlock => EWOULDBLOCK,
            ErrorKind::NotADirectory => ENOTDIR,
            ErrorKind::IsADirectory => EISDIR,
            ErrorKind::DirectoryNotEmpty => ENOTEMPTY,
            ErrorKind::ReadOnlyFilesystem => EROFS,
            ErrorKind::StaleNetworkFileHandle => ESTALE,
            ErrorKind::InvalidInput => EINVAL,
            ErrorKind::InvalidData => EINVAL,
            ErrorKind::TimedOut => ETIME,
            ErrorKind::WriteZero => EMSGSIZE,
            ErrorKind::StorageFull => ENOSPC,
            ErrorKind::NotSeekable => ESPIPE,
            ErrorKind::QuotaExceeded => EDQUOT,
            ErrorKind::FileTooLarge => EFBIG,
            ErrorKind::ResourceBusy => EBUSY,
            ErrorKind::ExecutableFileBusy => EBUSY,
            ErrorKind::Deadlock => EDEADLK,
            ErrorKind::CrossesDevices => EXDEV,
            ErrorKind::TooManyLinks => EMLINK,
            ErrorKind::InvalidFilename => EINVAL,
            ErrorKind::ArgumentListTooLong => E2BIG,
            ErrorKind::Interrupted => EINTR,
            ErrorKind::Unsupported => ENOTSUP,
            ErrorKind::UnexpectedEof => EIO,
            ErrorKind::OutOfMemory => ENOMEM,
            ErrorKind::Other => EIO,
            _ => EIO,
        }
    }
}

impl From<Errno> for i32 {
    fn from(e: Errno) -> i32 {
        e.0
    }
}

impl From<Errno> for std::io::Error {
    fn from(e: Errno) -> std::io::Error {
        std::io::Error::from_raw_os_error(e.0)
    }
}
