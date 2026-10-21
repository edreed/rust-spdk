use std::{
    marker::PhantomData,
    mem::transmute,
    ptr::{addr_of, addr_of_mut},
    slice,
};

use bitflags::bitflags;
use spdk_sys::{
    spdk_json_decode_bool, spdk_json_decode_int32, spdk_json_decode_uint8, spdk_json_decode_uint16,
    spdk_json_decode_uint32, spdk_json_decode_uint64, spdk_json_val, spdk_json_val_type,
};

use crate::{
    errors::{EINVAL, ERANGE, Errno},
    to_result, to_unexpected_value_result,
};

use super::{Error, Result};

/// Represents a JSON value type.
#[derive(PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct ValueType(u32);

bitflags! {
    impl ValueType : u32 {
        const Null = (1 << 1);
        const True = (1 << 2);
        const False = (1 << 3);
        const Number = (1 << 4);
        const String = (1 << 5);
        const ArrayBegin = (1 << 6);
        const ArrayEnd = (1 << 7);
        const ObjectBegin = (1 << 8);
        const ObjectEnd = (1 << 9);
        const Name = (1 << 10);

        const _ = !0;
    }
}

impl From<spdk_json_val_type> for ValueType {
    fn from(value: spdk_json_val_type) -> Self {
        unsafe { transmute(value as u32) }
    }
}

impl From<ValueType> for spdk_json_val_type {
    fn from(value: ValueType) -> Self {
        unsafe { transmute(value.0) }
    }
}
/// Represents a JSON value.
#[repr(transparent)]
pub(crate) struct Value<'de>(spdk_json_val, PhantomData<&'de str>);

impl<'de> Value<'de> {
    /// Converts an `Errno` to an appropriate [`Error`] value.
    fn errno_to_error(e: Errno) -> Error {
        match e {
            EINVAL => Error::UnexpectedValue,
            ERANGE => Error::ValueOutOfRange,
            _ => Error::ParseFailed,
        }
    }

    /// Returns the type of the JSON value.
    #[inline(always)]
    pub(crate) fn r#type(&self) -> ValueType {
        self.0.type_.into()
    }

    /// Returns whether the JSON value is null.
    pub(crate) fn is_null(&self) -> bool {
        self.r#type() == ValueType::Null
    }

    /// Returns whether the JSON value is a boolean.
    #[allow(dead_code)]
    pub(crate) fn is_bool(&self) -> bool {
        self.r#type().intersects(ValueType::True | ValueType::False)
    }

    /// Returns whether the JSON value is a number.
    #[allow(dead_code)]
    pub(crate) fn is_number(&self) -> bool {
        self.r#type() == ValueType::Number
    }

    /// Returns whether the JSON value is a string.
    pub(crate) fn is_string(&self) -> bool {
        self.r#type() == ValueType::String
    }

    /// Returns whether the JSON value is an object.
    pub(crate) fn is_object_begin(&self) -> bool {
        self.r#type() == ValueType::ObjectBegin
    }

    /// Returns whether the JSON value is the end of an object.
    pub(crate) fn is_object_end(&self) -> bool {
        self.r#type() == ValueType::ObjectEnd
    }

    /// Returns whether the JSON value is an array.
    pub(crate) fn is_array_begin(&self) -> bool {
        self.r#type() == ValueType::ArrayBegin
    }

    /// Returns whether the JSON value is the end of an array.
    pub(crate) fn is_array_end(&self) -> bool {
        self.r#type() == ValueType::ArrayEnd
    }

    /// Returns whether the JSON value is a name.
    #[allow(dead_code)]
    pub(crate) fn is_name(&self) -> bool {
        self.r#type() == ValueType::Name
    }

    /// Returns the string representation of the JSON value.
    pub(crate) fn to_str(&self) -> &'de str {
        unsafe {
            let s = slice::from_raw_parts(self.0.start.cast(), self.0.len as usize);

            str::from_utf8_unchecked(s)
        }
    }

    /// Returns a pointer to the JSON value.
    pub(crate) fn as_ptr(&self) -> *const spdk_json_val {
        addr_of!(self.0)
    }

    /// Returns the JSON value as a boolean.
    ///
    /// This method returns [`Error::UnexpectedValue`] if the JSON value is not
    /// a boolean.
    pub(crate) fn as_bool(&self) -> Result<bool> {
        let mut out = false;

        to_unexpected_value_result!(unsafe {
            spdk_json_decode_bool(self.as_ptr(), addr_of_mut!(out).cast())
        })?;

        Ok(out)
    }

    /// Returns the JSON value as a signed 32-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of a 32-bit integer.
    pub(crate) fn as_i32(&self) -> Result<i32> {
        let mut out = 0i32;

        to_result!(unsafe { spdk_json_decode_int32(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 8-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 8-bit integer.
    pub(crate) fn as_u8(&self) -> Result<u8> {
        let mut out = 0u8;

        to_result!(unsafe { spdk_json_decode_uint8(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 16-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 16-bit integer.
    pub(crate) fn as_u16(&self) -> Result<u16> {
        let mut out = 0u16;

        to_result!(unsafe { spdk_json_decode_uint16(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 32-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 32-bit integer.
    pub(crate) fn as_u32(&self) -> Result<u32> {
        let mut out = 0u32;

        to_result!(unsafe { spdk_json_decode_uint32(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 64-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 64-bit integer.
    pub(crate) fn as_u64(&self) -> Result<u64> {
        let mut out = 0u64;

        to_result!(unsafe { spdk_json_decode_uint64(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as a string.
    ///
    /// This method returns [`Error::UnexpectedValue`] if the JSON value is not
    /// a string or a name.
    pub(crate) fn as_str(&self) -> Result<&'de str> {
        if self
            .r#type()
            .intersects(ValueType::String | ValueType::Name)
        {
            return Ok(self.to_str());
        }

        Err(Error::UnexpectedValue)
    }
}
