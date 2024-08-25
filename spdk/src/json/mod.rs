#![cfg(feature = "json")]
//! JSON serialization and deserialization using the SPDK.
mod serde;

pub use serde::{
    de::Deserializer,
    error::Error,
    error::Result,
    ser::Serializer,

    de::from_str,
    ser::to_string,
};
