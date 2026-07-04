//! Storage Performance Development Kit ([SPDK]) FFI bindings for the Rust
//! programming language.
//!
//! [SPDK]: https://www.spdk.io
mod ffi {
    #![allow(clippy::doc_lazy_continuation)]
    #![allow(clippy::missing_safety_doc)]
    #![allow(clippy::ptr_offset_with_cast)]
    #![allow(clippy::too_many_arguments)]
    #![allow(clippy::type_complexity)]
    #![allow(clippy::unnecessary_cast)]
    #![allow(clippy::useless_transmute)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(non_upper_case_globals)]
    #![allow(unnecessary_transmutes)]

    #[cfg(feature = "nvmf")]
    mod nvmf {
        use bitfields::bitfield;

        /// Identify Controller Data Optional NVM Command Support
        #[bitfield(u16)]
        pub struct spdk_nvme_cdata_oncs {
            /// Compare Command Support
            #[bits(1)]
            nvmcmps: u16,
            /// Write Uncorrectable Support Variants
            #[bits(1)]
            nvmwusv: u16,
            /// Dataset Management Support Variants
            #[bits(1)]
            nvmdsmsv: u16,
            /// Write Zeros Support Variants
            #[bits(1)]
            nvmwzsv: u16,
            /// Save and Select Feature Support
            #[bits(1)]
            ssfs: u16,
            /// Reservation Support
            #[bits(1)]
            reservs: u16,
            /// Timestamp Support
            #[bits(1)]
            tss: u16,
            /// Verify Support
            #[bits(1)]
            nvmvfys: u16,
            /// Copy Support
            #[bits(1)]
            nvmcpys: u16,
            /// Reserved
            #[bits(7)]
            reserved: u16,
        }

        #[bitfield(u16)]
        pub struct spdk_nvme_cdata_fuses {
            /// Fused Compare and Write Supported
            #[bits(1)]
            fcws: u16,
            /// Reserved
            #[bits(15)]
            reserved: u16,
        }
    }

    #[cfg(feature = "nvmf")]
    use nvmf::*;

    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use ffi::*;
