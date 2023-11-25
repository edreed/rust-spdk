//! Command-line interface support for SPDK applications.
use crate::{
    errors::Errno,
    runtime::Builder,
};

pub use spdk_macros::Parser;

/// A trait for parsing command-line arguments.
pub trait Parser {
    /// Parsers the command-line arguments and returns a builder for the
    /// [`Runtime`].
    /// 
    /// [`Runtime`]: ../runtime/struct.Runtime.html
    fn parse() -> Result<Builder, Errno>;

    /// Returns a reference to the parsed command-line arguments.
    fn get() -> &'static Self;
}
