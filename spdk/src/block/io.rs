use std::{fmt::Debug, mem::transmute};

use spdk_sys::{
    spdk_bdev_io_status::{self, SPDK_BDEV_IO_STATUS_AIO_ERROR},
    spdk_bdev_io_type::{self, *},
};
use static_assertions::const_assert_eq;
use thiserror::Error;

use crate::errors::{ECANCELED, EINPROGRESS, ENOMEM, Errno};

#[cfg(feature = "nvmf")]
use crate::nvme::NvmeStatus;

#[cfg(feature = "scsi")]
use crate::scsi::ScsiStatus;

/// The type of an I/O operation.
///
/// # Notes
///
/// These are mapped directly to the corresponding [`spdk_bdev_io_type`] values.
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum IoType {
    Invalid,
    Read,
    Write,
    Unmap,
    Flush,
    Reset,
    NvmeAdmin,
    NvmeIo,
    NvmeIoMd,
    WriteZeros,
    ZeroCopy,
    GetZoneInfo,
    ZoneManagement,
    ZoneAppend,
    Compare,
    CompareAndWrite,
    Abort,
    SeekHole,
    SeekData,
    Copy,
    NvmeIovMd,
    NvmeNssr,
    WriteUncorrectable,
}

const_assert_eq!(IoType::Invalid as u32, SPDK_BDEV_IO_TYPE_INVALID as u32);
const_assert_eq!(IoType::Read as u32, SPDK_BDEV_IO_TYPE_READ as u32);
const_assert_eq!(IoType::Write as u32, SPDK_BDEV_IO_TYPE_WRITE as u32);
const_assert_eq!(IoType::Unmap as u32, SPDK_BDEV_IO_TYPE_UNMAP as u32);
const_assert_eq!(IoType::Flush as u32, SPDK_BDEV_IO_TYPE_FLUSH as u32);
const_assert_eq!(IoType::Reset as u32, SPDK_BDEV_IO_TYPE_RESET as u32);
const_assert_eq!(
    IoType::NvmeAdmin as u32,
    SPDK_BDEV_IO_TYPE_NVME_ADMIN as u32
);
const_assert_eq!(IoType::NvmeIo as u32, SPDK_BDEV_IO_TYPE_NVME_IO as u32);
const_assert_eq!(IoType::NvmeIoMd as u32, SPDK_BDEV_IO_TYPE_NVME_IO_MD as u32);
const_assert_eq!(
    IoType::WriteZeros as u32,
    SPDK_BDEV_IO_TYPE_WRITE_ZEROES as u32
);
const_assert_eq!(IoType::ZeroCopy as u32, SPDK_BDEV_IO_TYPE_ZCOPY as u32);
const_assert_eq!(
    IoType::GetZoneInfo as u32,
    SPDK_BDEV_IO_TYPE_GET_ZONE_INFO as u32
);
const_assert_eq!(
    IoType::ZoneManagement as u32,
    SPDK_BDEV_IO_TYPE_ZONE_MANAGEMENT as u32
);
const_assert_eq!(
    IoType::ZoneAppend as u32,
    SPDK_BDEV_IO_TYPE_ZONE_APPEND as u32
);
const_assert_eq!(IoType::Compare as u32, SPDK_BDEV_IO_TYPE_COMPARE as u32);
const_assert_eq!(
    IoType::CompareAndWrite as u32,
    SPDK_BDEV_IO_TYPE_COMPARE_AND_WRITE as u32
);
const_assert_eq!(IoType::Abort as u32, SPDK_BDEV_IO_TYPE_ABORT as u32);
const_assert_eq!(IoType::SeekHole as u32, SPDK_BDEV_IO_TYPE_SEEK_HOLE as u32);
const_assert_eq!(IoType::SeekData as u32, SPDK_BDEV_IO_TYPE_SEEK_DATA as u32);
const_assert_eq!(IoType::Copy as u32, SPDK_BDEV_IO_TYPE_COPY as u32);
const_assert_eq!(
    IoType::NvmeIovMd as u32,
    SPDK_BDEV_IO_TYPE_NVME_IOV_MD as u32
);
const_assert_eq!(IoType::NvmeNssr as u32, SPDK_BDEV_IO_TYPE_NVME_NSSR as u32);
const_assert_eq!(
    IoType::WriteUncorrectable as u32,
    SPDK_BDEV_IO_TYPE_WRITE_UNCORRECTABLE as u32
);
const_assert_eq!(SPDK_BDEV_NUM_IO_TYPES as u32, 23);

impl From<spdk_bdev_io_type> for IoType {
    fn from(value: spdk_bdev_io_type) -> Self {
        if value as u32 >= SPDK_BDEV_IO_TYPE_INVALID as u32
            || value as u32 <= SPDK_BDEV_IO_TYPE_WRITE_UNCORRECTABLE as u32
        {
            // SAFETY: The specified value is within the correct range for direct transmutation.
            return unsafe { transmute::<u8, Self>(value as u8) };
        }

        Self::Invalid
    }
}

impl From<u8> for IoType {
    fn from(value: u8) -> Self {
        if value >= Self::Invalid as u8 || value <= Self::WriteUncorrectable as u8 {
            // SAFETY: The specified value is within the correct range for direct transmutation.
            return unsafe { transmute::<u8, Self>(value) };
        }

        Self::Invalid
    }
}

impl From<IoType> for spdk_bdev_io_type {
    fn from(value: IoType) -> Self {
        // SAFETY: `IoType` has a 1:1 mapping to `spdk_bdev_io_type` values.
        unsafe { transmute(value as u32) }
    }
}

/// An error describing the reason for a BDev I/O failure.
#[derive(Copy, Clone, Debug, Error)]
pub enum IoError {
    /// A general error occurred.
    ///
    /// The `Errno` tuple field contains a Linux error code describing the reason.
    #[error(transparent)]
    GeneralError(Errno),

    /// The I/O was aborted.
    #[error("I/O aborted")]
    Aborted,

    /// The first fused request in a compare-and-write operation failed.
    #[error("the first fused request failed")]
    FirstFusedFailed,

    /// The block data in a compare or compare-and-write operation has changed.
    #[error("block data has changed")]
    Miscompare,

    /// There are currently no resources to submit a request.
    ///
    /// The request should be retried later when resources become available.
    #[error("out of resources")]
    NoMem,

    /// A SCSI error occurred.
    ///
    /// The `ScsiStatus` tuple field contains the SCSI status information.
    #[cfg(feature = "scsi")]
    #[error("a SCSI error occurred")]
    ScsiError(ScsiStatus),

    /// An NVMe error occurred.
    ///
    /// The `NvmeStatus` tuple field contains the NVMe status information.
    #[cfg(feature = "nvmf")]
    #[error("an NVME error occurred")]
    NvmeError(NvmeStatus),

    /// A general I/O failure occurred.
    #[error("I/O failed")]
    Failed,

    /// The I/O is still pending completion.
    #[error("I/O pending")]
    Pending,
}

// If this assertion failes, a new `spdk_bdev_io_status` may have been added and `IoError` should be
// updated accordingly.
const_assert_eq!(
    SPDK_BDEV_IO_STATUS_AIO_ERROR as i32,
    spdk_bdev_io_status::SPDK_MIN_BDEV_IO_STATUS as i32
);

impl From<Errno> for IoError {
    fn from(value: Errno) -> Self {
        match value {
            ENOMEM => Self::NoMem,
            ECANCELED => Self::Aborted,
            EINPROGRESS => Self::Pending,
            _ => Self::GeneralError(value),
        }
    }
}

pub type IoResult<T> = std::result::Result<T, IoError>;
