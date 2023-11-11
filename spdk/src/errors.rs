//! Common error definitions.
use libc;
use spdk_sys::Errno;

/// Operation not permitted
pub static EPERM: Errno = Errno(libc::EPERM);

/// No such file or directory
pub static ENOENT: Errno = Errno(libc::ENOENT);

/// Input/output error
pub static EIO: Errno = Errno(libc::EIO);

/// Bad file descriptor
pub static EBADF: Errno = Errno(libc::EBADF);

/// Not enough space/cannot allocate memory
pub static ENOMEM: Errno = Errno(libc::ENOMEM);

/// No such device
pub static ENODEV: Errno = Errno(libc::ENODEV);

/// Invalid argument
pub static EINVAL: Errno = Errno(libc::EINVAL);

/// Operation not supported
pub static ENOTSUP: Errno = Errno(libc::ENOTSUP);

/// Operation in progress
pub static EINPROGRESS: Errno = Errno(libc::EINPROGRESS);
