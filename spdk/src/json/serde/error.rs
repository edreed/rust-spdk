use core::fmt;

/// Error type for JSON serialization and deserialization.
#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    Message(String),
    WriteFailed,
    Eof,
    ParseFailed,
    UnexpectedValue,
    ValueOutOfRange,
}

impl std::error::Error for Error {}

impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Message(msg) => write!(fmt, "{}", msg),
            Error::WriteFailed => write!(fmt, "write failed"),
            Error::Eof => write!(fmt, "unexpected end of file"),
            Error::ParseFailed => write!(fmt, "parse failed"),
            Error::UnexpectedValue => write!(fmt, "unexpected value"),
            Error::ValueOutOfRange => write!(fmt, "value out of range"),
        }
    }
}

/// Result type for JSON serialization and deserialization.
pub type Result<T> = std::result::Result<T, Error>;
