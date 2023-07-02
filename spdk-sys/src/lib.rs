//! Storage Performance Development Kit ([SPDK]) FFI bindings for the Rust
//! programming language.
//! 
//! [SPDK]: https://www.spdk.io
pub mod macros;

mod ffi {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use ffi::*;
pub use errno::Errno;
