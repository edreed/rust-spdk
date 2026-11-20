//! Rust macro definitions to facilitate development of the `rust-spdk` crate's JSON serialization
//! and deserialization implementation.

/// Convert an SPDK integer status return value from an `spdk_json_write_xxx` function to a
/// [`Result`].
///
/// A status return value either indicates success (0) or an error (<0). A success return is
/// converted to `Ok(())` while an error return is converted to
/// `Err(spdk::json::serde::Error::WriteFailed)`.
#[macro_export]
#[doc(hidden)]
macro_rules! to_write_result {
    ($r:expr) => {
        if $r == 0 {
            Ok(())
        } else {
            Err($crate::json::serde::Error::WriteFailed)
        }
    };
}

/// Convert an SPDK integer status return value from an `spdk_json_decode_xxx` function to a
/// [`Result`].
///
/// A status return value either indicates success (0) or an error (<0). A success return is
/// converted to `Ok(())` while an error return is converted to
/// `Err(spdk::json::serde::Error::UnexpectedValue)`.
#[macro_export]
#[doc(hidden)]
macro_rules! to_unexpected_value_result {
    ($r:expr) => {
        if $r >= 0 {
            Ok(())
        } else {
            Err($crate::json::serde::Error::UnexpectedValue)
        }
    };
}
