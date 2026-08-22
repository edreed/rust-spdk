use std::{
    error::Error,
    fmt::{Debug, Display},
};

use spdk_sys::{
    spdk_nvme_command_specific_status_code::*, spdk_nvme_generic_command_status_code::*,
    spdk_nvme_media_error_status_code::*, spdk_nvme_path_status_code::*,
    spdk_nvme_status_code_type::*,
};

/// A type containing NVMe status information.
#[derive(Copy, Clone)]
pub struct NvmeStatus {
    /// NVMe command dword 0
    pub cdw0: u32,

    /// NVMe status code type
    pub sct: u8,

    /// NVMe status code
    pub sc: u8,
}

impl Debug for NvmeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NvmeStatus")
            .field("cdw0", &format_args!("{:x}", self.cdw0))
            .field("sct", &format_args!("{:x}", self.sct))
            .field("sc", &format_args!("{:x}", self.sc))
            .finish()
    }
}

impl Error for NvmeStatus {}

impl Display for NvmeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self.sct as u32 {
            SPDK_NVME_SCT_GENERIC => match self.sc as u32 {
                SPDK_NVME_SC_SUCCESS => "completed successfully",
                SPDK_NVME_SC_INVALID_OPCODE => "the command opcode is invalid",
                SPDK_NVME_SC_INVALID_FIELD => {
                    "an invalid or unsupported field is specified in the command parameters"
                }
                SPDK_NVME_SC_COMMAND_ID_CONFLICT => "the command identifier is already in use",
                SPDK_NVME_SC_DATA_TRANSFER_ERROR => {
                    "an error occurred transferring the data or metadata associated with a command"
                }
                SPDK_NVME_SC_ABORTED_POWER_LOSS => {
                    "the command was aborted due to a power loss notification"
                }
                SPDK_NVME_SC_INTERNAL_DEVICE_ERROR => {
                    "the command was not completed due to an internal device error"
                }
                SPDK_NVME_SC_ABORTED_BY_REQUEST => "the command was aborted by request",
                SPDK_NVME_SC_ABORTED_SQ_DELETION => {
                    "the command was aborted because its submission queue was deleted"
                }
                SPDK_NVME_SC_ABORTED_FAILED_FUSED => {
                    "the command was aborted because its companion fused command failed"
                }
                SPDK_NVME_SC_ABORTED_MISSING_FUSED => {
                    "the command was aborted because its companion fused command was not the next command in the submission queue"
                }
                SPDK_NVME_SC_INVALID_NAMESPACE_OR_FORMAT => {
                    "the namespace or its format is invalid"
                }
                SPDK_NVME_SC_COMMAND_SEQUENCE_ERROR => {
                    "the command was aborted due to a protocol violation in a multi-command sequence"
                }
                SPDK_NVME_SC_INVALID_SGL_SEG_DESCRIPTOR => {
                    "the command includes an invalid SGL descriptor"
                }
                SPDK_NVME_SC_INVALID_NUM_SGL_DESCIRPTORS => {
                    "the command includes a last SGL descriptor in the wrong position"
                }
                SPDK_NVME_SC_DATA_SGL_LENGTH_INVALID => {
                    "the command includes a data SGL with an invalid length"
                }
                SPDK_NVME_SC_METADATA_SGL_LENGTH_INVALID => {
                    "the command includes a metadata SGL with an invalid length"
                }
                SPDK_NVME_SC_SGL_DESCRIPTOR_TYPE_INVALID => {
                    "the command includes an unsupported SGL type"
                }
                SPDK_NVME_SC_INVALID_CONTROLLER_MEM_BUF => {
                    "use of the controller memory buffer is not supported"
                }
                SPDK_NVME_SC_INVALID_PRP_OFFSET => "a PRP entry contains an invalid offset",
                SPDK_NVME_SC_ATOMIC_WRITE_UNIT_EXCEEDED => {
                    "the length exceeds the atomic write unit size"
                }
                SPDK_NVME_SC_OPERATION_DENIED => {
                    "the command failed due to insufficient access rights"
                }
                SPDK_NVME_SC_INVALID_SGL_OFFSET => "an SGL entry contains an invalid offset",
                SPDK_NVME_SC_HOSTID_INCONSISTENT_FORMAT => {
                    "inconsistent use of 64-bit and 128-bit host identifier values on different controllers"
                }
                SPDK_NVME_SC_KEEP_ALIVE_EXPIRED => "the keep alive timer expired",
                SPDK_NVME_SC_KEEP_ALIVE_INVALID => "the keep alive timeout value is invalid",
                SPDK_NVME_SC_ABORTED_PREEMPT => {
                    "the command was preempted by a reservation command"
                }
                SPDK_NVME_SC_SANITIZE_FAILED => {
                    "the sanitize operation failed with no recovery action successfully completed"
                }
                SPDK_NVME_SC_SANITIZE_IN_PROGRESS => {
                    "the command failed because a sanitize operation is in progress"
                }
                SPDK_NVME_SC_SGL_DATA_BLOCK_GRANULARITY_INVALID => {
                    "an data block SGL entry has an invalid address alignment or length granularity"
                }
                SPDK_NVME_SC_COMMAND_INVALID_IN_CMB => {
                    "invalid command in the controller memory buffer"
                }
                SPDK_NVME_SC_COMMAND_NAMESPACE_IS_PROTECTED => "the command namespace is protected",
                SPDK_NVME_SC_COMMAND_INTERRUPTED => "the command was interrupted",
                SPDK_NVME_SC_COMMAND_TRANSIENT_TRANSPORT_ERROR => {
                    "the command failed due to a transient transport error"
                }
                SPDK_NVME_SC_COMMAND_PROHIBITED_BY_LOCKDOWN => {
                    "the command is prohibited by lockdown"
                }
                SPDK_NVME_SC_ADMIN_COMMAND_MEDIA_NOT_READY => {
                    "the administration command failed because the media is not ready"
                }
                SPDK_NVME_SC_FDP_DISABLED => "flexible data placement is disabled",
                SPDK_NVME_SC_INVALID_PLACEMENT_HANDLE_LIST => {
                    "invalid flexible data placement list"
                }
                SPDK_NVME_SC_LBA_OUT_OF_RANGE => "the logical block address is out of range",
                SPDK_NVME_SC_CAPACITY_EXCEEDED => "the namespace capacity was exceeded",
                SPDK_NVME_SC_NAMESPACE_NOT_READY => "the namespace is not ready",
                SPDK_NVME_SC_RESERVATION_CONFLICT => {
                    "a reservation is held on the accessed namespace"
                }
                SPDK_NVME_SC_FORMAT_IN_PROGRESS => "a format operation is in progress",
                SPDK_NVME_SC_INVALID_VALUE_SIZE => "the specified value size is invalid",
                SPDK_NVME_SC_INVALID_KEY_SIZE => "the specified key size is invalid",
                SPDK_NVME_SC_KV_KEY_DOES_NOT_EXIST => "the key does not exist",
                SPDK_NVME_SC_UNRECOVERED_ERROR => "an unrecoverable error occurred",
                SPDK_NVME_SC_KEY_EXISTS => "the key already exists",
                _ => {
                    return write!(
                        f,
                        "an unknown generic NVMe error occurred (code {:#x})",
                        self.sc
                    );
                }
            },
            SPDK_NVME_SCT_COMMAND_SPECIFIC => match self.sc as u32 {
                SPDK_NVME_SC_COMPLETION_QUEUE_INVALID => {
                    "the specified completion queue identifier is invalid"
                }
                SPDK_NVME_SC_INVALID_QUEUE_IDENTIFIER => {
                    "the specified queue identifier is invalid"
                }
                SPDK_NVME_SC_INVALID_QUEUE_SIZE => "the specified queue size is invalid",
                SPDK_NVME_SC_ABORT_COMMAND_LIMIT_EXCEEDED => {
                    "there are too many outstanding abort commands"
                }
                SPDK_NVME_SC_ASYNC_EVENT_REQUEST_LIMIT_EXCEEDED => {
                    "there are too many outstanding asynchronous event request commands"
                }
                SPDK_NVME_SC_INVALID_FIRMWARE_SLOT => {
                    "the specified firmware slot is invalid or read-only"
                }
                SPDK_NVME_SC_INVALID_FIRMWARE_IMAGE => "the specified firmware image is invalid",
                SPDK_NVME_SC_INVALID_INTERRUPT_VECTOR => {
                    "the specified interrupt vector is invalid"
                }
                SPDK_NVME_SC_INVALID_LOG_PAGE => "the specified log page is invalid",
                SPDK_NVME_SC_INVALID_FORMAT => "the specified format is invalid",
                SPDK_NVME_SC_FIRMWARE_REQ_CONVENTIONAL_RESET => {
                    "the firmware was committed successfully but requires a conventional reset to activate"
                }
                SPDK_NVME_SC_INVALID_QUEUE_DELETION => {
                    "the completion cannot be deleted while its submission queue exists"
                }
                SPDK_NVME_SC_FEATURE_ID_NOT_SAVEABLE => {
                    "the feature identifier does not support a saveable value"
                }
                SPDK_NVME_SC_FEATURE_NOT_CHANGEABLE => "the specified feature is not changeable",
                SPDK_NVME_SC_FEATURE_NOT_NAMESPACE_SPECIFIC => {
                    "the specified feature is not namespace-specific"
                }
                SPDK_NVME_SC_FIRMWARE_REQ_NVM_RESET => {
                    "the firmware was committed successfully but requires an NVM subsystem reset to activate"
                }
                SPDK_NVME_SC_FIRMWARE_REQ_RESET => {
                    "the firmware was committed successfully but requires an NVM controller reset to activate"
                }
                SPDK_NVME_SC_FIRMWARE_REQ_MAX_TIME_VIOLATION => {
                    "activating the firmware image would exceed the maximum activation time"
                }
                SPDK_NVME_SC_FIRMWARE_ACTIVATION_PROHIBITED => {
                    "the specified firmward image is prohibited from activation"
                }
                SPDK_NVME_SC_OVERLAPPING_RANGE => "the command specifies overlapping ranges",
                SPDK_NVME_SC_NAMESPACE_INSUFFICIENT_CAPACITY => {
                    "there is insufficient free space to create the namespace"
                }
                SPDK_NVME_SC_NAMESPACE_ID_UNAVAILABLE => {
                    "the number of supported namespaces has been exceeded"
                }
                SPDK_NVME_SC_NAMESPACE_ALREADY_ATTACHED => {
                    "the controller is already attached to the specified namespace"
                }
                SPDK_NVME_SC_NAMESPACE_IS_PRIVATE => {
                    "the specified namespace is private and attached to another controller"
                }
                SPDK_NVME_SC_NAMESPACE_NOT_ATTACHED => {
                    "the specified namespace is not attached to the controller"
                }
                SPDK_NVME_SC_THINPROVISIONING_NOT_SUPPORTED => "thin provisioning is not supported",
                SPDK_NVME_SC_CONTROLLER_LIST_INVALID => "the specified controller list is invalid",
                SPDK_NVME_SC_DEVICE_SELF_TEST_IN_PROGRESS => "a device self-test is in progress",
                SPDK_NVME_SC_BOOT_PARTITION_WRITE_PROHIBITED => {
                    "writing to the boot partition is prohibited"
                }
                SPDK_NVME_SC_INVALID_CTRLR_ID => "the specified controller identifier is invalid",
                SPDK_NVME_SC_INVALID_SECONDARY_CTRLR_STATE => {
                    "the state of the secondary controller is invalid"
                }
                SPDK_NVME_SC_INVALID_NUM_CTRLR_RESOURCES => {
                    "the specified number of controller resources is invalid"
                }
                SPDK_NVME_SC_INVALID_RESOURCE_ID => "the specified resource identifier is invalid",
                SPDK_NVME_SC_SANITIZE_PROHIBITED => "a sanitize command is prohibited",
                SPDK_NVME_SC_ANA_GROUP_IDENTIFIER_INVALID => {
                    "the specified ANA group identifier is invalid"
                }
                SPDK_NVME_SC_ANA_ATTACH_FAILED => {
                    "the controller could not be attached to the specified ANA group"
                }
                SPDK_NVME_SC_INSUFFICIENT_CAPACITY => {
                    "there is insufficient capacity to perform the command"
                }
                SPDK_NVME_SC_NAMESPACE_ATTACH_LIMIT_EXCEEDED => {
                    "the maximum number of namespace attachments has been exceeded"
                }
                SPDK_NVME_SC_PROHIBIT_CMD_EXEC_NOT_SUPPORTED => {
                    "prohibiting command execution is not supported"
                }
                SPDK_NVME_SC_IOCS_NOT_SUPPORTED => {
                    "the identify I/O command set command is not supported"
                }
                SPDK_NVME_SC_IOCS_NOT_ENABLED => {
                    "the identify I/O command set command is not enabled"
                }
                SPDK_NVME_SC_IOCS_COMBINATION_REJECTED => {
                    "the identify I/O command set command combination was rejected"
                }
                SPDK_NVME_SC_INVALID_IOCS => "the specified command set is invalid",
                SPDK_NVME_SC_IDENTIFIER_UNAVAILABLE => "the specified identifier is not available",
                SPDK_NVME_SC_STREAM_RESOURCE_ALLOCATION_FAILED => {
                    "stream resource allocation failed"
                }
                SPDK_NVME_SC_CONFLICTING_ATTRIBUTES => "conflicting attributes were specified",
                SPDK_NVME_SC_INVALID_PROTECTION_INFO => {
                    "the specified protection information was invalid"
                }
                SPDK_NVME_SC_ATTEMPTED_WRITE_TO_RO_RANGE => "the range is read-only",
                SPDK_NVME_SC_CMD_SIZE_LIMIT_SIZE_EXCEEDED => "the command size limit was exceeded",
                SPDK_NVME_SC_ZONED_BOUNDARY_ERROR => {
                    "the specified LBA range overlaps multiple zones"
                }
                SPDK_NVME_SC_ZONE_IS_FULL => "the zone is full",
                SPDK_NVME_SC_ZONE_IS_READ_ONLY => "the zone is read-only",
                SPDK_NVME_SC_ZONE_IS_OFFLINE => "the zone is offline",
                SPDK_NVME_SC_ZONE_INVALID_WRITE => "invalid write to zone",
                SPDK_NVME_SC_TOO_MANY_ACTIVE_ZONES => "there are too many active zones",
                SPDK_NVME_SC_TOO_MANY_OPEN_ZONES => "there are too many open zones",
                SPDK_NVME_SC_INVALID_ZONE_STATE_TRANSITION => {
                    "the requested zone state transition is invalid"
                }
                _ => {
                    return write!(
                        f,
                        "an unknown NVMe command-specific error occurred (code {:#x})",
                        self.sc
                    );
                }
            },
            SPDK_NVME_SCT_MEDIA_ERROR => match self.sc as u32 {
                SPDK_NVME_SC_WRITE_FAULTS => "the data could not be committed to the media",
                SPDK_NVME_SC_UNRECOVERED_READ_ERROR => {
                    "the data could not be recovered from the media"
                }
                SPDK_NVME_SC_GUARD_CHECK_ERROR => "an end-to-end guard check failure",
                SPDK_NVME_SC_APPLICATION_TAG_CHECK_ERROR => {
                    "an end-to-end application tag check failed"
                }
                SPDK_NVME_SC_REFERENCE_TAG_CHECK_ERROR => {
                    "an end-to-end reference tag check failed"
                }
                SPDK_NVME_SC_COMPARE_FAILURE => "the data has changed",
                SPDK_NVME_SC_ACCESS_DENIED => "access to the namespace or LBA range was denied",
                SPDK_NVME_SC_DEALLOCATED_OR_UNWRITTEN_BLOCK => {
                    "an attempt was made to read from an LBA range containing an unwritten or deallocate logical block"
                }
                SPDK_NVME_SC_END_TO_END_STORAGE_TAG_CHECK_ERROR => {
                    "an end-to-end storage tag check failed"
                }
                _ => {
                    return write!(
                        f,
                        "an unknown NVMe media error occurred (code {:#x})",
                        self.sc
                    );
                }
            },
            SPDK_NVME_SCT_PATH => match self.sc as u32 {
                SPDK_NVME_SC_INTERNAL_PATH_ERROR => {
                    "a multi-path internal error occurred at the controller"
                }
                SPDK_NVME_SC_ASYMMETRIC_ACCESS_PERSISTENT_LOSS => {
                    "the multi-path relationship between the controller and namespace has been lost"
                }
                SPDK_NVME_SC_ASYMMETRIC_ACCESS_INACCESSIBLE => {
                    "the multi-path relationship between the controller and namespace is inaccessible"
                }
                SPDK_NVME_SC_ASYMMETRIC_ACCESS_TRANSITION => {
                    "the multi-path relationship between the controller and namespace is transitioning states"
                }
                SPDK_NVME_SC_CONTROLLER_PATH_ERROR => {
                    "a multi-path error was detected by the controller"
                }
                SPDK_NVME_SC_HOST_PATH_ERROR => "a multi-path error was detected by the host",
                SPDK_NVME_SC_ABORTED_BY_HOST => "the multi-path command was aborted by the host",
                _ => {
                    return write!(
                        f,
                        "an unknown NVME multi-path error occurred (code {:#x})",
                        self.sc
                    );
                }
            },
            SPDK_NVME_SCT_VENDOR_SPECIFIC => {
                return write!(
                    f,
                    "a vendor-specific NVMe error occurred (code {:#x})",
                    self.sc
                );
            }
            _ => {
                return write!(
                    f,
                    "an unknown NVMe error occurred (type {:#x}, code {:#x})",
                    self.sct, self.sc
                );
            }
        };

        write!(f, "{msg}")
    }
}
