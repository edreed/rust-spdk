mod accessor;
mod deserializer;
mod tests;
mod value;

use super::{Error, Result};

pub use deserializer::Deserializer;

pub(crate) use accessor::Accessor;
pub(crate) use value::{Value, ValueType};

/// Deserializes a JSON string into a an instance of type `T`.
pub fn from_str<'de, T>(s: &'de str) -> Result<T>
where
    T: serde::de::Deserialize<'de>,
{
    let values = Value::from_str(s)?;
    let mut deserializer = Deserializer::new(&values);

    let t = T::deserialize(&mut deserializer)?;

    Ok(t)
}
