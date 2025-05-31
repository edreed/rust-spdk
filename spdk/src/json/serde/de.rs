use std::{
    marker::PhantomData,
    mem,
    os::raw::c_void,
    ptr::{self, addr_of, addr_of_mut},
    slice, str,
};

use serde::de::{self, IntoDeserializer};
use spdk_sys::{
    spdk_json_decode_bool, spdk_json_decode_int32, spdk_json_decode_uint16,
    spdk_json_decode_uint32, spdk_json_decode_uint64, spdk_json_decode_uint8, spdk_json_parse,
    spdk_json_val, spdk_json_val_type, SPDK_JSON_VAL_ARRAY_BEGIN, SPDK_JSON_VAL_ARRAY_END,
    SPDK_JSON_VAL_FALSE, SPDK_JSON_VAL_NAME, SPDK_JSON_VAL_NULL, SPDK_JSON_VAL_NUMBER,
    SPDK_JSON_VAL_OBJECT_BEGIN, SPDK_JSON_VAL_OBJECT_END, SPDK_JSON_VAL_STRING, SPDK_JSON_VAL_TRUE,
};

use crate::{
    errors::{self, Errno},
    to_result, to_unexpected_value_result,
};

use super::{Error, Result};

/// Represents a JSON value type.
type ValueType = spdk_json_val_type;

/// Represents a JSON value.
#[repr(transparent)]
struct Value<'a>(spdk_json_val, PhantomData<&'a spdk_json_val>);

impl<'de> Value<'de> {
    /// Converts an `Errno` to an appropriate [`Error`] value.
    fn errno_to_error(e: Errno) -> Error {
        match e {
            errors::EINVAL => Error::UnexpectedValue,
            errors::ERANGE => Error::ValueOutOfRange,
            _ => Error::ParseFailed,
        }
    }

    /// Returns the type of the JSON value.
    #[inline(always)]
    fn r#type(&self) -> ValueType {
        self.0.type_
    }

    /// Returns whether the JSON value is null.
    fn is_null(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_NULL
    }

    /// Returns whether the JSON value is a boolean.
    #[allow(dead_code)]
    fn is_bool(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_TRUE || self.r#type() == SPDK_JSON_VAL_FALSE
    }

    /// Returns whether the JSON value is a number.
    #[allow(dead_code)]
    fn is_number(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_NUMBER
    }

    /// Returns whether the JSON value is a string.
    fn is_string(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_STRING
    }

    /// Returns whether the JSON value is an object.
    fn is_object_begin(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_OBJECT_BEGIN
    }

    /// Returns whether the JSON value is the end of an object.
    fn is_object_end(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_OBJECT_END
    }

    /// Returns whether the JSON value is an array.
    fn is_array_begin(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_ARRAY_BEGIN
    }

    /// Returns whether the JSON value is the end of an array.
    fn is_array_end(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_ARRAY_END
    }

    /// Returns whether the JSON value is a name.
    #[allow(dead_code)]
    fn is_name(&self) -> bool {
        self.r#type() == SPDK_JSON_VAL_NAME
    }

    /// Returns the string representation of the JSON value.
    fn to_str(&self) -> &'de str {
        unsafe {
            let s = slice::from_raw_parts(self.0.start.cast(), self.0.len as usize);

            str::from_utf8_unchecked(s)
        }
    }

    /// Returns a pointer to the JSON value.
    fn as_ptr(&self) -> *const spdk_json_val {
        addr_of!(self.0)
    }

    /// Returns the JSON value as a boolean.
    ///
    /// This method returns [`Error::UnexpectedValue`] if the JSON value is not
    /// a boolean.
    fn as_bool(&self) -> Result<bool> {
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
    fn as_i32(&self) -> Result<i32> {
        let mut out = 0i32;

        to_result!(unsafe { spdk_json_decode_int32(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 8-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 8-bit integer.
    fn as_u8(&self) -> Result<u8> {
        let mut out = 0u8;

        to_result!(unsafe { spdk_json_decode_uint8(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 16-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 16-bit integer.
    fn as_u16(&self) -> Result<u16> {
        let mut out = 0u16;

        to_result!(unsafe { spdk_json_decode_uint16(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 32-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 32-bit integer.
    fn as_u32(&self) -> Result<u32> {
        let mut out = 0u32;

        to_result!(unsafe { spdk_json_decode_uint32(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as an unsigned 64-bit integer.
    ///
    /// This method returns an error if the JSON value is not a number or does
    /// not fit into the range of an unsigned 64-bit integer.
    fn as_u64(&self) -> Result<u64> {
        let mut out = 0u64;

        to_result!(unsafe { spdk_json_decode_uint64(self.as_ptr(), addr_of_mut!(out).cast()) })
            .map_err(Self::errno_to_error)?;

        Ok(out)
    }

    /// Returns the JSON value as a string.
    ///
    /// This method returns [`Error::UnexpectedValue`] if the JSON value is not
    /// a string or a name.
    fn as_str(&self) -> Result<&'de str> {
        match self.r#type() {
            SPDK_JSON_VAL_STRING | SPDK_JSON_VAL_NAME => Ok(self.to_str()),
            _ => Err(Error::UnexpectedValue),
        }
    }
}

const DEFAULT_CAPACITY: usize = 256;

/// Deserializes a JSON string into a Rust data structure.
pub struct Deserializer<'de> {
    values: Vec<Value<'de>>,
    position: usize,
}

impl<'de> Deserializer<'de> {
    /// Creates a new deserializer from a JSON string.
    pub fn try_from_str(input: &'de str) -> Result<Self> {
        let mut values: Vec<Value<'de>> = Vec::with_capacity(DEFAULT_CAPACITY);

        loop {
            // SAFETY: `Value` is a transparent wrapper around `spdk_json_val`
            // and `values` is a `Vec` of `Value`s with a non-zero capacity, so
            // the pointer is valid.
            let rc = unsafe {
                spdk_json_parse(
                    input.as_ptr() as *mut u8 as *mut c_void,
                    input.len(),
                    mem::transmute(values.as_mut_ptr()),
                    values.capacity(),
                    ptr::null_mut(),
                    0,
                )
            };

            if rc < 0 {
                return Err(Error::ParseFailed);
            }

            let len = rc as usize;

            if len <= values.capacity() {
                // SAFETY: `spdk_json_parse` has written 'len' elements and
                // `len` is less than or equal to the capacity of `values`.
                unsafe { values.set_len(len) };
                break;
            }

            values.reserve(len - values.capacity());
        }

        Ok(Self {
            position: 0,
            values,
        })
    }

    /// Returns the next JSON value without consuming it.
    #[must_use]
    fn peek(&self) -> Result<&Value<'de>> {
        self.values.get(self.position).ok_or(Error::Eof)
    }

    /// Returns the next JSON value and advances the position.
    #[must_use]
    fn next(&mut self) -> Result<&Value<'de>> {
        if self.position < self.values.len() {
            let current = self.position;

            self.position += 1;

            return Ok(&self.values[current]);
        }

        Err(Error::Eof)
    }

    /// Discards the next JSON value.
    fn discard_next(&mut self) {
        if self.position < self.values.len() {
            self.position += 1;
        }
    }
}

impl<'a, 'de> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let v = self.next()?;

        match v.r#type() {
            SPDK_JSON_VAL_NULL => self.deserialize_option(visitor),
            SPDK_JSON_VAL_TRUE | SPDK_JSON_VAL_FALSE => self.deserialize_bool(visitor),
            SPDK_JSON_VAL_STRING => self.deserialize_str(visitor),
            SPDK_JSON_VAL_ARRAY_BEGIN => self.deserialize_seq(visitor),
            SPDK_JSON_VAL_OBJECT_BEGIN => self.deserialize_map(visitor),
            SPDK_JSON_VAL_OBJECT_END | SPDK_JSON_VAL_ARRAY_END => Err(Error::UnexpectedValue),
            _ => self.deserialize_str(visitor),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_bool()?;

        visitor.visit_bool(v)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self
            .next()?
            .as_i32()?
            .try_into()
            .map_err(|_| Error::ValueOutOfRange)?;

        visitor.visit_i8(v)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self
            .next()?
            .as_i32()?
            .try_into()
            .map_err(|_| Error::ValueOutOfRange)?;

        visitor.visit_i16(v)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_i32()?;

        visitor.visit_i32(v)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self
            .next()?
            .to_str()
            .parse()
            .map_err(|_| Error::ValueOutOfRange)?;

        visitor.visit_i64(v)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_u8()?;

        visitor.visit_u8(v)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_u16()?;

        visitor.visit_u16(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_u32()?;

        visitor.visit_u32(v)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_u64()?;

        visitor.visit_u64(v)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self
            .next()?
            .to_str()
            .parse()
            .map_err(|_| Error::ValueOutOfRange)?;

        visitor.visit_f32(v)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self
            .next()?
            .to_str()
            .parse()
            .map_err(|_| Error::ValueOutOfRange)?;

        visitor.visit_f64(v)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_str()?;

        if v.len() != 1 {
            return Err(Error::ValueOutOfRange);
        }

        visitor.visit_char(v.chars().next().unwrap())
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?.as_str()?;

        visitor.visit_borrowed_str(v)
    }

    #[inline(always)]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    #[inline(always)]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline(always)]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        if self.peek()?.is_null() {
            self.discard_next();
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?;

        if !v.is_object_begin() {
            return Err(Error::UnexpectedValue);
        }

        let v = self.next()?;

        if !v.is_object_end() {
            return Err(Error::UnexpectedValue);
        }

        visitor.visit_unit()
    }

    #[inline(always)]
    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    #[inline(always)]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?;

        if !v.is_array_begin() {
            return Err(Error::ParseFailed);
        }

        visitor.visit_seq(Accessor::new(self))
    }

    #[inline(always)]
    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline(always)]
    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?;

        if !v.is_object_begin() {
            return Err(Error::ParseFailed);
        }

        visitor.visit_map(Accessor::new(self))
    }

    #[inline(always)]
    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        let v = self.next()?;

        if v.is_string() {
            return visitor.visit_enum(v.as_str().unwrap().into_deserializer());
        }

        visitor.visit_enum(Accessor::new(self))
    }

    #[inline(always)]
    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }
}

struct Accessor<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
}

impl<'a, 'de> Accessor<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>) -> Self {
        Self { de }
    }
}

impl<'a, 'de> de::SeqAccess<'de> for Accessor<'a, 'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        let v = self.de.peek()?;

        if v.is_array_end() {
            self.de.discard_next();
            Ok(None)
        } else if !v.is_object_end() {
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Err(Error::ParseFailed)
        }
    }
}

impl<'a, 'de> de::MapAccess<'de> for Accessor<'a, 'de> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: de::DeserializeSeed<'de>,
    {
        let v = self.de.peek()?;

        if v.is_object_end() {
            self.de.discard_next();
            Ok(None)
        } else if !v.is_array_end() {
            seed.deserialize(&mut *self.de).map(Some)
        } else {
            Err(Error::ParseFailed)
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }
}

impl<'de, 'a> de::EnumAccess<'de> for Accessor<'a, 'de> {
    type Error = Error;
    type Variant = Self;

    #[inline(always)]
    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de).map(|v| (v, self))
    }
}

impl<'de, 'a> de::VariantAccess<'de> for Accessor<'a, 'de> {
    type Error = Error;

    #[inline(always)]
    fn unit_variant(self) -> Result<()> {
        de::Deserialize::deserialize(&mut *self.de)
    }

    #[inline(always)]
    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.de)
    }

    #[inline(always)]
    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(&mut *self.de, visitor)
    }

    #[inline(always)]
    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_struct(&mut *self.de, "", fields, visitor)
    }
}

/// Deserializes a JSON string into a an instance of type `T`.
pub fn from_str<'de, T>(s: &'de str) -> Result<T>
where
    T: de::Deserialize<'de>,
{
    let mut deserializer = Deserializer::try_from_str(s)?;

    let t = T::deserialize(&mut deserializer)?;

    Ok(t)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use laboratory::{describe, expect, LabResult, NullState};

    use crate::json::serde::test::{
        TestEnum, TestNewTypeStruct, TestStruct, TestTupleStruct, TestUnitStruct,
    };

    use super::*;

    #[test]
    fn suite() -> LabResult {
        describe("to_string()", |suite| {
            suite
                .it(r#"should deserialize "true" to true"#, |_| {
                    expect(from_str("true")).to_equal(Ok(true))
                })
                .it(r#"should deserialize "false" to false"#, |_| {
                    expect(from_str("false")).to_equal(Ok(false))
                })
                .it("should deserialize a number to a i8", |_| {
                    expect(from_str("42")).to_equal(Ok(42i8))
                })
                .it("should deserialize a negative number to a i8", |_| {
                    expect(from_str("-42")).to_equal(Ok(-42i8))
                })
                .it("should deserialize a number to i16", |_| {
                    expect(from_str("42")).to_equal(Ok(42i16))
                })
                .it("should deserialize a negative number to i16", |_| {
                    expect(from_str("-42")).to_equal(Ok(-42i16))
                })
                .it("should deserialize a number to i32", |_| {
                    expect(from_str("42")).to_equal(Ok(42i32))
                })
                .it("should deserialize a negative number to i32", |_| {
                    expect(from_str("-42")).to_equal(Ok(-42i32))
                })
                .it("should deserialize a number to i64", |_| {
                    expect(from_str("42")).to_equal(Ok(42i64))
                })
                .it("should deserialize a negative number to i64", |_| {
                    expect(from_str("-42")).to_equal(Ok(-42i64))
                })
                .it("should deserialize a number to u8", |_| {
                    expect(from_str("42")).to_equal(Ok(42u8))
                })
                .it("should deserialize a number to u16", |_| {
                    expect(from_str("42")).to_equal(Ok(42u16))
                })
                .it("should deserialize a number to u32", |_| {
                    expect(from_str("42")).to_equal(Ok(42u32))
                })
                .it("should deserialize a number to u64", |_| {
                    expect(from_str("42")).to_equal(Ok(42u64))
                })
                .it("should deserialize a number to f32", |_| {
                    expect(from_str("4.20000000000000000000e+01")).to_equal(Ok(42.0f32))
                })
                .it("should deserialize a number to f64", |_| {
                    expect(from_str("4.20000000000000000000e+01")).to_equal(Ok(42.0f64))
                })
                .it("should deserialize single char string to a char", |_| {
                    expect(from_str(r#""a""#)).to_equal(Ok('a'))
                })
                .it("should deserialize string to a string", |_| {
                    expect(from_str(r#""foo""#)).to_equal(Ok("foo".to_string()))
                })
                .it("should deserialize an array to a [u8]", |_| {
                    expect(from_str("[2,3,7]")).to_equal(Ok([2u8, 3u8, 7u8]))
                })
                .it("should deserialize null to None", |_| {
                    expect(from_str("null")).to_equal(Ok(Option::<i32>::None))
                })
                .it("should deserialize a number to Some(i32)", |_| {
                    expect(from_str("237")).to_equal(Ok(Some(237i32)))
                })
                .it("should deserialize an array to a Vec", |_| {
                    expect(from_str("[2,3,7]")).to_equal(Ok(vec![2u16, 3u16, 7u16]))
                })
                .it("should deserialize an array to a tuple", |_| {
                    expect(from_str(r#"[237,"foo",true]"#)).to_equal(Ok((237, "foo", true)))
                })
                .it("should deserialize an empty object to unit", |_| {
                    expect(from_str("{}")).to_equal(Ok(()))
                })
                .it("should deserialize a string to a unit variant", |_| {
                    expect(from_str(r#""Unit""#)).to_equal(Ok(TestEnum::Unit))
                })
                .it("should deserialize an object to a newtype variant", |_| {
                    expect(from_str(r#"{"NewType":237}"#)).to_equal(Ok(TestEnum::NewType(237)))
                })
                .it("should deserialize an object to a tuple variant", |_| {
                    expect(from_str(r#"{"Tuple":[237,"foo",true]}"#))
                        .to_equal(Ok(TestEnum::Tuple(237, "foo", true)))
                })
                .it("should deserialize an object to a struct variant", |_| {
                    expect(from_str(r#"{"Struct":{"foo":"bar","bar":237}}"#)).to_equal(Ok(
                        TestEnum::Struct {
                            foo: "bar".to_string(),
                            bar: 237,
                        },
                    ))
                })
                .it(
                    "should deserialize an empty object to a unit struct",
                    |_| expect(from_str("{}")).to_equal(Ok(TestUnitStruct)),
                )
                .it("should deserialize a number to a newtype struct", |_| {
                    expect(from_str("237")).to_equal(Ok(TestNewTypeStruct(237)))
                })
                .it("should deserialize an array to a tuple struct", |_| {
                    expect(from_str(r#"[237,"foo",true]"#))
                        .to_equal(Ok(TestTupleStruct(237, "foo", true)))
                })
                .it("should deserialize an object to a struct", |_| {
                    expect(from_str(r#"{"foo":"bar","bar":237}"#)).to_equal(Ok(TestStruct {
                        foo: "bar".to_string(),
                        bar: 237,
                    }))
                })
                .it("should deserialize an object to a map", |_| {
                    expect(from_str(r#"{"bar":42,"foo":237}"#))
                        .to_equal(Ok(BTreeMap::from([("bar", 42i32), ("foo", 237i32)])))
                });
        })
        .state(NullState)
        .run()
    }
}
