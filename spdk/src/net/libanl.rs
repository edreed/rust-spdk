#![allow(non_camel_case_types)]
use std::{ffi::{
    c_char,
    c_int,
    CStr,
}, fmt::Debug};

use libc::{
    addrinfo,
    gai_strerror,
    sigevent,
    timespec,
};

#[repr(C)]
pub struct gaicb {
    pub ar_name: *const c_char,
    pub ar_service: *const c_char,
    pub ar_request: *const addrinfo,
    pub ar_result: *mut addrinfo,
    pub __return: c_int,
    pub reserved: [c_int; 5],
}

pub const GAI_WAIT: c_int = 0;
pub const GAI_NOWAIT: c_int = 1;

#[link(name = "anl")]   // Link to the system libanl library
extern "C" {
    pub fn getaddrinfo_a(mode: c_int, list: *mut *mut gaicb, nitems: c_int, sevp: *mut sigevent) -> c_int;

    pub fn gai_suspend(list: *const *const gaicb, nitems: c_int, timeout: *const timespec) -> c_int;

    pub fn gai_error(req: *const gaicb) -> c_int;

    pub fn gai_cancel(req: *mut gaicb) -> c_int;
}

/// The error type for `getaddrinfo`, `getaddrinfo_a` and related asynchronous
/// network library calls.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Error(pub c_int);

impl ToString for Error {
    fn to_string(&self) -> String {
        unsafe { CStr::from_ptr(gai_strerror(self.0)) }.to_string_lossy().into_owned()
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error {{ code: {}, description: \"{}\" }}", self.0, self.to_string())
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
            rc => Err(crate::net::libanl::Error(rc)),
        }
    };
}