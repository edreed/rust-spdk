use libc;
use spdk_sys::Errno;

pub static ENOMEM: Errno = Errno(libc::ENOMEM);
pub static EIO: Errno = Errno(libc::EIO);