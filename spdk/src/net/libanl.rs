//! FFI bindings for the Asynchronous Name Lookup library.
#![allow(non_camel_case_types)]
use core::fmt;
use std::{
    ffi::{c_char, c_int, CStr},
    fmt::{Debug, Display, Formatter},
};

use libc::{addrinfo, gai_strerror, sigevent, timespec};

/// Specifies the Internet host and service to lookup in a call to the [`getaddrinfo_a`] function.
#[repr(C)]
pub struct gaicb {
    /// The name of the Internet host to look up.
    pub ar_name: *const c_char,
    /// The service to look up. This may be a port number or service name, e.g. `"http"`, `"ftp"`, etc.
    pub ar_service: *const c_char,
    /// An optional pointer to an [`addrinfo`] structure that specifies the criteria for selecting
    /// the socket address structure returned in `ar_result`. If `null`, the criteria is equivalent
    /// to passing `ai_socktype` and `ai_protocol` to zero; `ai_family` to `AF_UNSPEC` and
    /// `ai_flags` to `(AI_V4MAPPED | AI_ADDR_CONFIG)`.
    ///
    /// See the [`getaddrinfo`] documentation on the `hint` parameter for more information.
    ///
    /// [`getaddrinfo`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo.3.html
    pub ar_request: *const addrinfo,
    /// A pointer to an [`addrinfo`] structure that receives the result of the operation.
    ///
    /// [`addrinfo`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo.3.html
    pub ar_result: *mut addrinfo,
    __return: c_int,
    reserved: [c_int; 5],
}

pub const GAI_WAIT: c_int = 0;
pub const GAI_NOWAIT: c_int = 1;

#[link(name = "anl")] // Link to the system libanl library
extern "C" {
    /// Returns one or more [`addrinfo`] structures for the specified Internet hosts and services.
    ///
    /// # Parameters
    ///
    /// - `mode`:  The mode argument has one of the following values:
    ///     - [`GAI_WAIT`]: Perform the lookups synchronously. The call blocks until the lookups
    ///       have completed.
    ///     - [`GAI_NOWAIT`]: Perform the lookups asynchronously. The call returns immediately, and
    ///       the requests are resolved in the background. See the `sevp` argument for details
    ///       on how to receive notification of completion.
    /// - `list`: An array of pointers to [`gaicb`] elements specifying the lookup requests to process.
    /// - `nitems`: Specifies the number of elements in the `list` argument.
    /// - `sevp`: If `mode` is `GAI_NOWAIT`, an optional pointer `sigevent` structure that specifies how to
    ///   notify the process of completion. See the [`sigevent`] documentation for more
    ///   information. If `null`, no notification is delivered. In this case, you may call
    ///   [`gai_suspend`] to wait for results. In either case, [`gai_error`] must be called on
    ///   each item in `list` to receive the status of the lookup request.
    ///
    /// # Returns
    ///
    /// See [`getaddrinfo_a`] for possible return values.
    ///
    /// [`addrinfo`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo.3.html
    /// [`getaddrinfo_a`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo_a.3.html
    /// [`sigevent`]: https://www.man7.org/linux/man-pages/man3/sigevent.3type.html
    pub fn getaddrinfo_a(
        mode: c_int,
        list: *mut *mut gaicb,
        nitems: c_int,
        sevp: *mut sigevent,
    ) -> c_int;

    /// Suspends the execution of the calling thread, waiting for completion of one or more request
    /// in the array `list`.
    ///
    /// # Parameters
    ///
    /// - `list`: A pointer to the array of [`gaicb`] elements passed to the [`getaddrinfo_a`] function.
    /// - `nitems`: Specifies the number of elements in the `list` argument.
    /// - `timemout`: The duration to wait for results. See [`timespec`] for more information.
    ///
    /// # Returns
    ///
    /// See the [`getaddrinfo_a`] documentation for possible return values.
    ///
    /// [`getaddrinfo_a`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo_a.3.html
    /// [`timespec`]: https://www.man7.org/linux/man-pages/man3/timespec.3type.html
    pub fn gai_suspend(list: *const *const gaicb, nitems: c_int, timeout: *const timespec)
        -> c_int;

    /// Returns the status of a request in a call to the [`getaddrinfo_a`] function.
    ///
    /// # Returns
    ///
    /// See the [`getaddrinfo_a`] documentation for possible return values.
    ///
    /// [`getaddrinfo_a`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo_a.3.html
    pub fn gai_error(req: *const gaicb) -> c_int;

    /// Cancels a request initiated in a call to the [`getaddrinfo_a`] function.
    ///
    /// # Returns
    ///
    /// See the [`getaddrinfo_a`] documentation for possible return values.
    ///
    /// [`getaddrinfo_a`]: https://www.man7.org/linux/man-pages/man3/getaddrinfo_a.3.html
    pub fn gai_cancel(req: *mut gaicb) -> c_int;
}

/// The error type for `getaddrinfo`, `getaddrinfo_a` and related asynchronous
/// network library calls.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Error(pub c_int);

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = unsafe { CStr::from_ptr(gai_strerror(self.0)) }.to_string_lossy();

        write!(f, "{}", s)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error {{ code: {}, description: \"{}\" }}", self.0, self)
    }
}

pub const EAI_BADFLAGS: Error = Error(-1);
pub const EAI_NONAME: Error = Error(-2);
pub const EAI_AGAIN: Error = Error(-3);
pub const EAI_FAIL: Error = Error(-4);
pub const EAI_NODATA: Error = Error(-5);
pub const EAI_FAMILY: Error = Error(-6);
pub const EAI_SOCKTYPE: Error = Error(-7);
pub const EAI_SERVICE: Error = Error(-8);
pub const EAI_ADDRFAMILY: Error = Error(-9);
pub const EAI_MEMORY: Error = Error(-10);
pub const EAI_SYSTEM: Error = Error(-11);
pub const EAI_INPROGRESS: Error = Error(-100);

/// Converts the return value from a `getaddrinfo` family function into an
/// [`Error`] value.
#[macro_export]
macro_rules! eai_to_result {
    ($e:expr) => {
        match $e {
            0 => Ok(()),
            rc => Err($crate::net::libanl::Error(rc)),
        }
    };
}
