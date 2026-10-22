use std::fmt;

use crate::errors::{EINVAL, ERANGE, Errno, ErrorCode};

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

impl ErrorCode for Error {
    fn code(&self) -> i32 {
        match self {
            Self::Message(_) => -1,
            Self::WriteFailed => -2,
            Self::Eof => -3,
            Self::ParseFailed => -4,
            Self::UnexpectedValue => -5,
            Self::ValueOutOfRange => -6,
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
