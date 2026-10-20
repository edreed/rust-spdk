mod name_serializer;
mod serializer;
mod tests;

use std::io::Write;

use serde::Serialize;

use super::{Error, Result};

pub use serializer::Serializer;

/// Serializes the Rust value to JSON writing the result to the specified writer.
pub fn to_writer<T, W>(value: &T, writer: &mut W) -> Result<()>
where
    T: Serialize + ?Sized,
    W: Write,
{
    let mut ser = Serializer::new(writer)?;

    value.serialize(&mut ser)?;
    ser.finish()
}

/// Serializes the Rust value to a JSON string.
pub fn to_string<T>(value: &T) -> Result<String>
where
    T: Serialize + ?Sized,
{
    let mut buf = Vec::new();

    to_writer(value, &mut buf)?;

    Ok(String::from_utf8_lossy_owned(buf))
}
