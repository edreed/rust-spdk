use std::fmt;

use spdk_sys::{
    SPDK_JSONRPC_ERROR_INTERNAL_ERROR, SPDK_JSONRPC_ERROR_INVALID_PARAMS,
    SPDK_JSONRPC_ERROR_PARSE_ERROR,
};

use crate::errors::{EINVAL, ERANGE, Errno};

/// Error type for JSON serialization and deserialization.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error("failed to write JSON")]
    WriteFailed,

    #[error("end of file")]
    Eof,

    #[error("failed to parse JSON")]
    ParseFailed,

    #[error("unexpected value")]
    UnexpectedValue,

    #[error("value out of range")]
    ValueOutOfRange,
}

impl Error {
    pub(crate) fn json_rpc_code(&self) -> i32 {
        match self {
            Self::Message(_) | Self::WriteFailed => SPDK_JSONRPC_ERROR_INTERNAL_ERROR,
            Self::Eof | Self::ParseFailed => SPDK_JSONRPC_ERROR_PARSE_ERROR,
            Self::UnexpectedValue | Self::ValueOutOfRange => SPDK_JSONRPC_ERROR_INVALID_PARAMS,
        }
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl From<Errno> for Error {
    fn from(e: Errno) -> Self {
        match e {
            EINVAL => Error::UnexpectedValue,
            ERANGE => Error::ValueOutOfRange,
            _ => Error::ParseFailed,
        }
    }
}

/// Result type for JSON serialization and deserialization.
pub type Result<T> = std::result::Result<T, Error>;
