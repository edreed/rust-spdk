use std::{
    error::Error,
    fmt::{Debug, Display},
};

use spdk_sys::{spdk_scsi_asc::*, spdk_scsi_ascq::*, spdk_scsi_sense::*, spdk_scsi_status::*};

/// A type containing SCSI status information.
#[derive(Copy, Clone)]
pub struct ScsiStatus {
    /// SCSI status code
    pub sc: u8,

    /// SCSI sense key
    pub sk: u8,

    /// SCSI additional sense code
    pub asc: u8,

    /// SCSI additional sense code qualifier
    pub ascq: u8,
}

impl Debug for ScsiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScsiStatus")
            .field("sc", &format_args!("{:x}", self.sc))
            .field("sk", &format_args!("{:x}", self.sk))
            .field("asc", &format_args!("{:x}", self.asc))
            .field("ascq", &format_args!("{:x}", self.ascq))
            .finish()
    }
}

impl Error for ScsiStatus {}

impl Display for ScsiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self.sc as u32 {
            SPDK_SCSI_STATUS_GOOD => Some("the command was completed successfully"),
            SPDK_SCSI_STATUS_CHECK_CONDITION => match self.sk as u32 {
                SPDK_SCSI_SENSE_NO_SENSE => match self.asc as u32 {
                    SPDK_SCSI_ASC_NO_ADDITIONAL_SENSE => Some("the command failed"),
                    _ => None,
                },
                SPDK_SCSI_SENSE_NOT_READY => match self.asc as u32 {
                    SPDK_SCSI_ASC_NO_ADDITIONAL_SENSE => Some("the device is not ready"),
                    SPDK_SCSI_ASC_LOGICAL_UNIT_NOT_READY => Some("the logical unit is not ready"),
                    _ => None,
                },
                SPDK_SCSI_SENSE_HARDWARE_ERROR => match self.asc as u32 {
                    SPDK_SCSI_ASC_INTERNAL_TARGET_FAILURE => {
                        Some("the command was not completed due to an internal device error")
                    }
                    _ => None,
                },
                SPDK_SCSI_SENSE_ILLEGAL_REQUEST => match self.asc as u32 {
                    SPDK_SCSI_ASC_INVALID_COMMAND_OPERATION_CODE => {
                        Some("the command opcode is invalid")
                    }
                    SPDK_SCSI_ASC_LOGICAL_BLOCK_ADDRESS_OUT_OF_RANGE => {
                        Some("the logical block address is out of range")
                    }
                    SPDK_SCSI_ASC_INVALID_FIELD_IN_CDB => Some(
                        "an invalid or unsupported field is specified in the command parameters",
                    ),
                    SPDK_SCSI_ASC_LOGICAL_UNIT_NOT_SUPPORTED => {
                        Some("the logical unit is not supported")
                    }
                    SPDK_SCSI_ASC_SAVING_PARAMETERS_NOT_SUPPORTED => {
                        Some("saving parameters is not supported")
                    }
                    _ => None,
                },
                SPDK_SCSI_SENSE_UNIT_ATTENTION => match self.asc as u32 {
                    SPDK_SCSI_ASC_CAPACITY_DATA_HAS_CHANGED => {
                        Some("the logical unit capacity has changed")
                    }
                    _ => None,
                },
                SPDK_SCSI_SENSE_MISCOMPARE => match self.asc as u32 {
                    SPDK_SCSI_ASC_MISCOMPARE_DURING_VERIFY_OPERATION => {
                        Some("the data has changed")
                    }
                    _ => None,
                },
                _ => None,
            },
            SPDK_SCSI_STATUS_RESERVATION_CONFLICT => {
                Some("the reservation request comflicts with another reservation")
            }
            SPDK_SCSI_STATUS_TASK_ABORTED => match self.sk as u32 {
                SPDK_SCSI_SENSE_ABORTED_COMMAND => match self.ascq as u32 {
                    SPDK_SCSI_ASCQ_POWER_LOSS_EXPECTED => {
                        Some("the command was aborted because a power loss was detected")
                    }
                    _ => Some("the command was aborted"),
                },
                _ => None,
            },
            _ => None,
        };

        if let Some(msg) = msg {
            write!(f, "{}", msg)
        } else {
            write!(
                f,
                "an unknown SCSI error occurred (code {:#x}, sense {:#x}, asc {:#x}, ascq {:#x})",
                self.sc, self.sk, self.asc, self.ascq
            )
        }
    }
}
