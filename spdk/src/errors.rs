use libc;
use spdk_sys::Errno;

/// Operation not permitted
pub static EPERM: Errno = Errno(libc::EPERM);

/// Input/output error
pub static EIO: Errno = Errno(libc::EIO);

/// Bad file descriptor
pub static EBADF: Errno = Errno(libc::EBADF);

/// Not enough space/cannot allocate memory
pub static ENOMEM: Errno = Errno(libc::ENOMEM);

/// Invalid argument
pub static EINVAL: Errno = Errno(libc::EINVAL);

/// Operation not supported
pub static ENOTSUP: Errno = Errno(libc::ENOTSUP);
