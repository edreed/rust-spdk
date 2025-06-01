//! Storage Performance Development Kit ([SPDK]) FFI bindings for the Rust
//! programming language.
//!
//! [SPDK]: https://www.spdk.io
mod ffi {
    #![allow(clippy::missing_safety_doc)]
    #![allow(clippy::ptr_offset_with_cast)]
    #![allow(clippy::too_many_arguments)]
    #![allow(clippy::type_complexity)]
    #![allow(clippy::unnecessary_cast)]
    #![allow(clippy::useless_transmute)]
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use ffi::*;
