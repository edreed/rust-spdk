//! Common error definitions.
use libc;

pub use errno::Errno;

/// Operation not permitted
pub const EPERM: Errno = Errno(libc::EPERM);

/// No such file or directory
pub const ENOENT: Errno = Errno(libc::ENOENT);

/// Input/output error
pub const EIO: Errno = Errno(libc::EIO);

/// Bad file descriptor
pub const EBADF: Errno = Errno(libc::EBADF);

/// Try again
pub const EAGAIN: Errno = Errno(libc::EAGAIN);

/// Not enough space/cannot allocate memory
pub const ENOMEM: Errno = Errno(libc::ENOMEM);

/// No such device
pub const ENODEV: Errno = Errno(libc::ENODEV);

/// Invalid argument
pub const EINVAL: Errno = Errno(libc::EINVAL);

/// No data avaiable
pub const ENODATA: Errno = Errno(libc::ENODATA);

/// Protocol wrong type for socket
pub const EPROTOTYPE: Errno = Errno(libc::EPROTOTYPE);

/// Socket type not supported
pub const ESOCKTNOSUPPORT: Errno = Errno(libc::ESOCKTNOSUPPORT);

/// Operation not supported
pub const ENOTSUP: Errno = Errno(libc::ENOTSUP);

/// Address family not supported
pub const EAFNOSUPPORT: Errno = Errno(libc::EAFNOSUPPORT);

/// The socket is not connected
pub const ENOTCONN: Errno = Errno(libc::ENOTCONN);

/// Operation already in progress
pub const EALREADY: Errno = Errno(libc::EALREADY);

/// Operation in progress
pub const EINPROGRESS: Errno = Errno(libc::EINPROGRESS);

/// Operation canceled
pub const ECANCELED: Errno = Errno(libc::ECANCELED);

#[cfg(feature = "net")]
impl From<crate::net::libanl::Error> for Errno {
    fn from(e: crate::net::libanl::Error) -> Self {
        match e {
            crate::net::libanl::EAI_BADFLAGS => EINVAL,
            crate::net::libanl::EAI_NONAME => ENOENT,
            crate::net::libanl::EAI_AGAIN => EAGAIN,
            crate::net::libanl::EAI_FAIL => EIO,
            crate::net::libanl::EAI_NODATA => ENODATA,
            crate::net::libanl::EAI_FAMILY => EAFNOSUPPORT,
            crate::net::libanl::EAI_SOCKTYPE => EPROTOTYPE,
            crate::net::libanl::EAI_SERVICE => ESOCKTNOSUPPORT,
            crate::net::libanl::EAI_ADDRFAMILY => EAFNOSUPPORT,
            crate::net::libanl::EAI_MEMORY => ENOMEM,
            crate::net::libanl::EAI_SYSTEM => errno::errno(),
            crate::net::libanl::EAI_INPROGRESS => EINPROGRESS,
            _ => EINVAL,
        }
    }
}