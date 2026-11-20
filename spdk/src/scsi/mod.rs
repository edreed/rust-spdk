//! Support for SCSI features of the Storage Performance Development Kit.
#![cfg(feature = "scsi")]
#![cfg_attr(docsrs, doc(cfg(feature = "scsi")))]

mod status;

pub use status::ScsiStatus;
