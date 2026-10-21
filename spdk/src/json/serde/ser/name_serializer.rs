use std::{marker::PhantomData, mem::transmute, ptr::NonNull};

use serde::{Serialize, ser};
use spdk_sys::{spdk_json_write_ctx, spdk_json_write_name_raw};

use crate::to_write_result;

use super::{Error, Result, Serializer};

/// Serializes a JSON name.
#[repr(transparent)]
pub(crate) struct NameSerializer<'ser, W> {
    writer: NonNull<spdk_json_write_ctx>,
    _marker: PhantomData<&'ser mut Serializer<'ser, W>>,
}

impl<'ser, W> NameSerializer<'ser, W> {
    /// Creates a new `NameSerializer`.
    pub(crate) fn new(ser: &'ser mut Serializer<W>) -> Self {
        unsafe { transmute(ser.0) }
    }
}

impl<W> ser::Serializer for &mut NameSerializer<'_, W> {
    type Ok = ();

    type Error = Error;

    type SerializeSeq = ser::Impossible<(), Error>;
    type SerializeTuple = ser::Impossible<(), Error>;
    type SerializeTupleStruct = ser::Impossible<(), Error>;
    type SerializeTupleVariant = ser::Impossible<(), Error>;
    type SerializeMap = ser::Impossible<(), Error>;
    type SerializeStruct = ser::Impossible<(), Error>;
    type SerializeStructVariant = ser::Impossible<(), Error>;

    #[inline(always)]
    fn serialize_bool(self, _v: bool) -> Result<Self::Ok> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_i8(self, _v: i8) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_i16(self, _v: i16) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_i32(self, _v: i32) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_i64(self, _v: i64) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_u8(self, _v: u8) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_u16(self, _v: u16) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_u32(self, _v: u32) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_u64(self, _v: u64) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_f32(self, _v: f32) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_f64(self, _v: f64) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_char(self, _v: char) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_str(self, v: &str) -> std::result::Result<Self::Ok, Self::Error> {
        to_write_result!(unsafe {
            spdk_json_write_name_raw(self.writer.as_ptr(), v.as_ptr().cast(), v.len())
        })
    }

    #[inline(always)]
    fn serialize_bytes(self, _v: &[u8]) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_none(self) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_some<T>(self, _v: &T) -> std::result::Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_unit(self) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_unit_struct(
        self,
        _name: &'static str,
    ) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> std::result::Result<Self::Ok, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> std::result::Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> std::result::Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_seq(
        self,
        _len: Option<usize>,
    ) -> std::result::Result<Self::SerializeSeq, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_tuple(
        self,
        _len: usize,
    ) -> std::result::Result<Self::SerializeTuple, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_map(
        self,
        _len: Option<usize>,
    ) -> std::result::Result<Self::SerializeMap, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeStruct, Self::Error> {
        Err(Error::WriteFailed)
    }

    #[inline(always)]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> std::result::Result<Self::SerializeStructVariant, Self::Error> {
        Err(Error::WriteFailed)
    }
}
