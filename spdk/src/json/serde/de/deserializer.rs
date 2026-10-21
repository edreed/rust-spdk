use std::{mem::transmute, os::raw::c_void, ptr::null_mut};

use serde::de::IntoDeserializer;
use spdk_sys::{spdk_json_parse, spdk_json_val};

use super::{Accessor, Error, Result, Value, ValueType};

const DEFAULT_CAPACITY: usize = 256;

/// Deserializes a JSON string into a Rust data structure.
pub struct Deserializer<'de> {
    values: Vec<Value<'de>>,
    position: usize,
}

impl<'de> Deserializer<'de> {
    /// Creates a new deserializer from a JSON string.
    pub fn new(input: &'de str) -> Result<Self> {
        let mut values: Vec<Value<'de>> = Vec::with_capacity(DEFAULT_CAPACITY);

        loop {
            // SAFETY: `Value` is a transparent wrapper around `spdk_json_val`
            // and `values` is a `Vec` of `Value`s with a non-zero capacity, so
            // the pointer is valid.
            let rc = unsafe {
                spdk_json_parse(
                    input.as_ptr() as *mut u8 as *mut c_void,
                    input.len(),
                    transmute::<*mut Value<'_>, *mut spdk_json_val>(values.as_mut_ptr()),
                    values.capacity(),
                    null_mut(),
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
    pub(crate) fn peek(&self) -> Result<&Value<'de>> {
        self.values.get(self.position).ok_or(Error::Eof)
    }

    /// Returns the next JSON value and advances the position.
    pub(crate) fn next(&mut self) -> Result<&Value<'de>> {
        if self.position < self.values.len() {
            let current = self.position;

            self.position += 1;

            return Ok(&self.values[current]);
        }

        Err(Error::Eof)
    }

    /// Discards the next JSON value.
    pub(crate) fn discard_next(&mut self) {
        if self.position < self.values.len() {
            self.position += 1;
        }
    }
}

impl<'de> serde::de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: serde::de::Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let v = self.next()?;

        match v.r#type() {
            ValueType::Null => self.deserialize_option(visitor),
            ValueType::True | ValueType::False => self.deserialize_bool(visitor),
            ValueType::String => self.deserialize_str(visitor),
            ValueType::ArrayBegin => self.deserialize_seq(visitor),
            ValueType::ObjectBegin => self.deserialize_map(visitor),
            ValueType::ArrayEnd | ValueType::ObjectEnd => Err(Error::UnexpectedValue),
            _ => self.deserialize_str(visitor),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_bool()?;

        visitor.visit_bool(v)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_i32()?;

        visitor.visit_i32(v)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_u8()?;

        visitor.visit_u8(v)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_u16()?;

        visitor.visit_u16(v)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_u32()?;

        visitor.visit_u32(v)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_u64()?;

        visitor.visit_u64(v)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_str()?;

        if v.len() != 1 {
            return Err(Error::ValueOutOfRange);
        }

        visitor.visit_char(v.chars().next().unwrap())
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        let v = self.next()?.as_str()?;

        visitor.visit_borrowed_str(v)
    }

    #[inline(always)]
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    #[inline(always)]
    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline(always)]
    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    #[inline(always)]
    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
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
        V: serde::de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: serde::de::Visitor<'de>,
    {
        todo!()
    }
}
