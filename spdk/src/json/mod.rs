#![cfg(feature = "json")]
//! JSON serialization and deserialization using the SPDK.
mod serde;

pub use serde::{
    de::from_str, de::Deserializer, error::Error, error::Result, ser::to_string, ser::Serializer,
};
