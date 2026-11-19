//! # [Data Integrity Field (DIF)] Support
//!
//! The Data Integrity Field (DIF) protects data integrity in a block storage system. When enabled,
//! an additional 8 or 16 bytes are attached to each logical block. These bytes contain three
//! fields:
//!
//! ```text
//! +-------+-----+-----+
//! | GUARD | APP | REF |
//! +-------+-----+-----+
//! ```
//!
//! The GUARD field contains a 16-, 32-, or 64--bit checksum of the logical block data. The APP
//! field contains an arbitrary application reference tag. The REF field contains a logical block
//! reference tag which is generally the lower 32-, 48- or 64-bits of the logical block address. The
//! [`PiFormat`] determines the size of the fields.
//!
//! [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
use std::{mem::transmute, result};

use bitflags::bitflags;
use spdk_sys::{
    SPDK_DIF_FLAGS_APPTAG_CHECK, SPDK_DIF_FLAGS_GUARD_CHECK, SPDK_DIF_FLAGS_NVME_PRACT,
    SPDK_DIF_FLAGS_REFTAG_CHECK,
    spdk_dif_check_type::{self, *},
    spdk_dif_pi_format::{self, *},
    spdk_dif_type::{self, *},
};
use static_assertions::const_assert_eq;

use crate::errors::UnknownEnumVariantError;

/// An enumeration of [Data Integrity Field (DIF)] types.
///
/// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    /// The device does not support DIF or it is disabled.
    Disabled,

    /// The device implements DIF type 1 protection.
    Type1,

    /// The device implements DIF type 2 protection.
    Type2,

    /// The device implements DIF type 3 protection.
    Type3,
}

const_assert_eq!(Type::Disabled as u32, SPDK_DIF_DISABLE as u32);
const_assert_eq!(Type::Type1 as u32, SPDK_DIF_TYPE1 as u32);
const_assert_eq!(Type::Type2 as u32, SPDK_DIF_TYPE2 as u32);
const_assert_eq!(Type::Type3 as u32, SPDK_DIF_TYPE3 as u32);

impl From<spdk_dif_type> for Type {
    fn from(value: spdk_dif_type) -> Self {
        if value as u32 >= SPDK_DIF_DISABLE as u32 && value as u32 <= SPDK_DIF_TYPE3 as u32 {
            // SAFETY: The specified value is within the correct range for direct transmutation.
            return unsafe { transmute::<u8, Self>(value as u8) };
        }

        Self::Disabled
    }
}

impl From<Type> for spdk_dif_type {
    fn from(value: Type) -> Self {
        // SAFETY: `Type` has a 1:1 mapping to `spdk_dif_type` values.
        unsafe { transmute(value as u32) }
    }
}

/// An enumeration of [Data Integrity Field (DIF)] protection information formats.
///
/// [Data Integrity Field (DIF)]: https://en.wikipedia.org/wiki/Data_Integrity_Field
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiFormat {
    /// The DIF field is 8 bytes in size with 16-bit GUARD, 16-bit APP and 32-bit REF fields.
    ///
    ///```text
    /// +------------+---------+---------+
    /// | GUARD (16) | APP(16) | REF(32) |
    /// +------------+---------+---------+
    /// ```
    Guard16,

    /// The DIF field is 16 bytes in size with 32-bit GUARD, 16-bit APP and 64-bit REF fields, and
    /// 16-bits of unused space.
    ///
    /// ```text
    /// +------------+---------+------------+---------+
    /// | GUARD (32) | APP(16) | UNUSED(16) | REF(64) |
    /// +------------+---------+------------+---------+
    /// ```
    Guard32,

    /// The DIF field is 16 bytes in size with 64-bit GUARD, 16-bit APP and 48-bit REF fields.
    ///
    ///
    /// ```text
    /// +------------+---------+---------+
    /// | GUARD (64) | APP(16) | REF(48) |
    /// +------------+---------+---------+
    /// ```
    Guard64,
}

const_assert_eq!(PiFormat::Guard16 as u32, SPDK_DIF_PI_FORMAT_16 as u32);
const_assert_eq!(PiFormat::Guard32 as u32, SPDK_DIF_PI_FORMAT_32 as u32);
const_assert_eq!(PiFormat::Guard64 as u32, SPDK_DIF_PI_FORMAT_64 as u32);

impl TryFrom<spdk_dif_pi_format> for PiFormat {
    type Error = UnknownEnumVariantError<u32>;

    fn try_from(value: spdk_dif_pi_format) -> result::Result<Self, Self::Error> {
        if value as u32 >= SPDK_DIF_PI_FORMAT_16 as u32
            && value as u32 <= SPDK_DIF_PI_FORMAT_64 as u32
        {
            // SAFETY: The specified value is within the correct range for direct transmutation.
            return Ok(unsafe { transmute::<u8, Self>(value as u8) });
        }

        Err(UnknownEnumVariantError {
            enum_name: "spdk_dif_pi_format",
            variant_value: value as u32,
        })
    }
}

impl From<PiFormat> for spdk_dif_pi_format {
    fn from(value: PiFormat) -> Self {
        // SAFETY: `PiFormat` has a 1:1 mapping to `spdk_dif_pi_format` values.
        unsafe { transmute(value as u32) }
    }
}

bitflags! {
    /// Flags that determine what [Data Integrity Field (DIF)] checks are performed.
    pub struct CheckFlag : u32 {
        /// If set, check the REF tag.
        const RefTag = SPDK_DIF_FLAGS_REFTAG_CHECK;

        /// If set, check the APP tag.
        const AppTag = SPDK_DIF_FLAGS_APPTAG_CHECK;

        /// If set, check the GUARD checksum.
        const Guard = SPDK_DIF_FLAGS_GUARD_CHECK;

        /// If set, perform the NVMe protection information action (PRACT).
        ///
        /// When enabled, the protection information is stripped from the block when read and
        /// inserted when written if the block format's metadata size matches the expected format
        /// size.
        const NvmePract = SPDK_DIF_FLAGS_NVME_PRACT;
    }
}

/// An enumeration of DIF check types that can be queried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckType {
    /// Query whether the REF tag is checked.
    RefTag = 1,

    /// Query whether the APP tag is checked.
    AppTag = 2,

    /// Query whether the GUARD checksum is checked.
    Guard = 3,
}

const_assert_eq!(CheckType::RefTag as u32, SPDK_DIF_CHECK_TYPE_REFTAG as u32);
const_assert_eq!(CheckType::AppTag as u32, SPDK_DIF_CHECK_TYPE_APPTAG as u32);
const_assert_eq!(CheckType::Guard as u32, SPDK_DIF_CHECK_TYPE_GUARD as u32);

impl TryFrom<spdk_dif_check_type> for CheckType {
    type Error = UnknownEnumVariantError<u32>;

    fn try_from(value: spdk_dif_check_type) -> result::Result<Self, Self::Error> {
        if value as u32 >= SPDK_DIF_CHECK_TYPE_REFTAG as u32
            && value as u32 <= SPDK_DIF_CHECK_TYPE_GUARD as u32
        {
            // SAFETY: The specified value is within the correct range for direct transmutation.
            return Ok(unsafe { transmute::<u8, Self>(value as u8) });
        }

        Err(UnknownEnumVariantError {
            enum_name: "spdk_dif_check_type",
            variant_value: value as u32,
        })
    }
}

impl From<CheckType> for spdk_dif_check_type {
    fn from(value: CheckType) -> Self {
        // SAFETY: `CheckType` has a 1:1 mapping to `spdk_dif_check_type` values.
        unsafe { transmute(value as u32) }
    }
}
