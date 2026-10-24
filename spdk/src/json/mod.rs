#![cfg(feature = "json")]
//! JSON serialization and deserialization using the SPDK.
mod rpc;
mod serde;

pub use serde::{
    de::Deserializer, de::from_str, error::Error, error::Result, ser::Serializer, ser::to_string,
};

#[cfg(feature = "json-rpc")]
pub use rpc::{Infallible as RpcInfallible, RpcCall, rpc_call};
