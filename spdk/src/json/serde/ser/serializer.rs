use core::slice;
use std::{
    fmt::Debug,
    io::Write,
    marker::PhantomData,
    mem::transmute,
    os::raw::{c_int, c_void},
    ptr::NonNull,
};

use serde::ser;
use spdk_sys::{
    spdk_json_write_array_begin, spdk_json_write_array_end, spdk_json_write_begin,
    spdk_json_write_bool, spdk_json_write_bytearray, spdk_json_write_ctx, spdk_json_write_double,
    spdk_json_write_end, spdk_json_write_int32, spdk_json_write_int64, spdk_json_write_name_raw,
    spdk_json_write_null, spdk_json_write_object_begin, spdk_json_write_object_end,
    spdk_json_write_string_raw, spdk_json_write_uint8, spdk_json_write_uint16,
    spdk_json_write_uint32, spdk_json_write_uint64,
};

use crate::{
    errors::{EIO, SUCCESS},
    to_write_result,
};

use super::{Error, Result, name_serializer::NameSerializer};

/// Serializes a Rust data structure into a JSON string.
#[repr(transparent)]
#[derive(Debug)]
pub struct Serializer<'ser, W>(
    pub(crate) NonNull<spdk_json_write_ctx>,
    pub(crate) PhantomData<&'ser mut W>,
);

impl<'ser, W: Write> Serializer<'ser, W> {
    /// Creates a new `Serializer` writing JSON to the specified writer.
    pub fn new(writer: &'ser mut W) -> Result<Self> {
        NonNull::new(unsafe {
            spdk_json_write_begin(Some(Self::write_json), writer as *mut _ as *mut _, 0)
        })
        .map_or(Err(Error::WriteFailed), |w| {
            Ok(unsafe { transmute::<NonNull<spdk_json_write_ctx>, Self>(w) })
        })
    }

    /// A callback function invoked to write JSON data to the buffer.
    unsafe extern "C" fn write_json(ctx: *mut c_void, data: *const c_void, size: usize) -> c_int {
        let writer = unsafe { &mut *(ctx as *mut W) };
        let buf = unsafe { slice::from_raw_parts(data as *const u8, size) };

        writer.write_all(buf).map_or_else(
            |e| -e.raw_os_error().unwrap_or(EIO.into()),
            |_| SUCCESS.into(),
        )
    }

    /// Consumes the `Serializer` and finishes writing the JSON data.
    pub fn finish(self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_end(self.0.as_ptr()) })
    }
}

impl<'ser, W> Serializer<'ser, W> {
    /// Begins writing an object.
    #[inline(always)]
    fn begin_object(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_object_begin(self.0.as_ptr()) })
    }

    /// Ends writing an object.
    #[inline(always)]
    fn end_object(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_object_end(self.0.as_ptr()) })
    }

    /// Begins writing an array.
    #[inline(always)]
    fn begin_array(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_array_begin(self.0.as_ptr()) })
    }

    /// Ends writing an array.
    #[inline(always)]
    fn end_array(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_array_end(self.0.as_ptr()) })
    }

    /// Writes a name.
    #[inline(always)]
    fn write_name(&mut self, name: &str) -> Result<()> {
        to_write_result!(unsafe {
            spdk_json_write_name_raw(self.0.as_ptr(), name.as_ptr().cast(), name.len())
        })
    }
}

impl<'ser, W> ser::Serializer for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    #[inline(always)]
    fn serialize_bool(self, v: bool) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_bool(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_i8(self, v: i8) -> Result<Self::Ok> {
        self.serialize_i32(v as i32)
    }

    #[inline(always)]
    fn serialize_i16(self, v: i16) -> Result<Self::Ok> {
        self.serialize_i32(v as i32)
    }

    #[inline(always)]
    fn serialize_i32(self, v: i32) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_int32(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_i64(self, v: i64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_int64(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u8(self, v: u8) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint8(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u16(self, v: u16) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint16(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u32(self, v: u32) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint32(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u64(self, v: u64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint64(self.0.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_f32(self, v: f32) -> Result<Self::Ok> {
        self.serialize_f64(v as f64)
    }

    #[inline(always)]
    fn serialize_f64(self, v: f64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_double(self.0.as_ptr(), v) })
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok> {
        let mut buf = [0; 4];
        let s = v.encode_utf8(&mut buf);

        to_write_result!(unsafe {
            spdk_json_write_string_raw(self.0.as_ptr(), s.as_ptr().cast(), s.len())
        })
    }

    #[inline(always)]
    fn serialize_str(self, v: &str) -> Result<Self::Ok> {
        to_write_result!(unsafe {
            spdk_json_write_string_raw(self.0.as_ptr(), v.as_ptr().cast(), v.len())
        })
    }

    #[inline(always)]
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok> {
        to_write_result!(unsafe {
            spdk_json_write_bytearray(self.0.as_ptr(), v.as_ptr().cast(), v.len())
        })
    }

    #[inline(always)]
    fn serialize_none(self) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_null(self.0.as_ptr()) })
    }

    #[inline(always)]
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(self)
    }

    #[inline(always)]
    fn serialize_unit(self) -> Result<Self::Ok> {
        let s = self.serialize_map(Some(0))?;
        ser::SerializeMap::end(s)
    }

    #[inline(always)]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.serialize_unit()
    }

    #[inline(always)]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok> {
        self.serialize_str(variant)
    }

    #[inline(always)]
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        self.begin_object()?;
        self.write_name(variant)?;

        value.serialize(&mut *self)?;

        self.end_object()
    }

    #[inline(always)]
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        self.begin_array()?;

        Ok(self)
    }

    #[inline(always)]
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    #[inline(always)]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.begin_object()?;
        self.write_name(variant)?;
        self.begin_array()?;

        Ok(self)
    }

    #[inline(always)]
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        self.begin_object()?;

        Ok(self)
    }

    #[inline(always)]
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.begin_object()?;
        self.write_name(variant)?;
        self.begin_object()?;

        Ok(self)
    }
}

impl<'ser, W> ser::SerializeSeq for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_element<T>(&mut self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_array()
    }
}

impl<'ser, W> ser::SerializeTuple for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_element<T>(&mut self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_array()
    }
}

impl<'ser, W> ser::SerializeTupleStruct for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_field<T>(&mut self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_array()
    }
}

impl<'ser, W> ser::SerializeTupleVariant for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_field<T>(&mut self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_array()?;
        self.end_object()
    }
}

impl<'ser, W> ser::SerializeStruct for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        self.write_name(key)?;
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_object()
    }
}

impl<'ser, W> ser::SerializeStructVariant for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        self.write_name(key)?;
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_object()?;
        self.end_object()
    }
}

impl<'ser, W> ser::SerializeMap for &mut Serializer<'ser, W> {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_key<T>(&mut self, key: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        key.serialize(&mut NameSerializer::new(self))
    }

    #[inline(always)]
    fn serialize_value<T>(&mut self, value: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline(always)]
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        self.end_object()
    }
}
