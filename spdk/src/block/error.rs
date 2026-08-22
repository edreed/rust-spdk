use std::fmt::Debug;

use thiserror::Error;

use crate::errors::{ECANCELED, EINPROGRESS, ENOMEM, Errno};

#[cfg(feature = "nvmf")]
use crate::nvme::NvmeStatus;

#[cfg(feature = "scsi")]
use crate::scsi::ScsiStatus;

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
