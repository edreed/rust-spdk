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

/// Not enough space/cannot allocate memory
pub const ENOMEM: Errno = Errno(libc::ENOMEM);

/// No such device
pub const ENODEV: Errno = Errno(libc::ENODEV);

/// Invalid argument
pub const EINVAL: Errno = Errno(libc::EINVAL);

/// Operation not supported
pub const ENOTSUP: Errno = Errno(libc::ENOTSUP);

/// Operation already in progress
pub const EALREADY: Errno = Errno(libc::EALREADY);

/// Operation in progress
pub const EINPROGRESS: Errno = Errno(libc::EINPROGRESS);

/// Operation canceled
pub const ECANCELED: Errno = Errno(libc::ECANCELED);
