use std::{
    mem,
    os::raw::{c_int, c_void},
    ptr::NonNull,
};

use serde::{ser, Serialize};
use spdk_sys::{
    spdk_json_write_array_begin, spdk_json_write_array_end, spdk_json_write_begin,
    spdk_json_write_bool, spdk_json_write_bytearray, spdk_json_write_ctx, spdk_json_write_double,
    spdk_json_write_end, spdk_json_write_int32, spdk_json_write_int64, spdk_json_write_name_raw,
    spdk_json_write_null, spdk_json_write_object_begin, spdk_json_write_object_end,
    spdk_json_write_string_raw, spdk_json_write_uint16, spdk_json_write_uint32,
    spdk_json_write_uint64, spdk_json_write_uint8,
};

use crate::to_write_result;

use super::{Error, Result};

/// Serializes a JSON name.
struct NameSerializer {
    writer: NonNull<spdk_json_write_ctx>,
}

impl NameSerializer {
    /// Creates a new `NameSerializer`.
    fn new(writer: NonNull<spdk_json_write_ctx>) -> Self {
        Self { writer }
    }
}

impl ser::Serializer for &mut NameSerializer {
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

/// Inner state of a `Serializer`.
struct SerializerInner {
    writer: NonNull<spdk_json_write_ctx>,
    buffer: String,
}

impl SerializerInner {
    /// Creates a new `SerializerInner`.
    pub fn try_new() -> Result<Box<Self>> {
        let mut inner = Box::new(Self {
            writer: NonNull::dangling(),
            buffer: String::new(),
        });

        match NonNull::new(unsafe {
            spdk_json_write_begin(
                Some(Self::write_buffer),
                inner.as_mut() as *mut _ as *mut c_void,
                0,
            )
        }) {
            Some(writer) => {
                inner.writer = writer;
                Ok(inner)
            }
            None => Err(Error::WriteFailed),
        }
    }

    /// A callback function invoked to write data to the buffer.
    unsafe extern "C" fn write_buffer(ctx: *mut c_void, data: *const c_void, size: usize) -> c_int {
        let inner = &mut *(ctx as *mut SerializerInner);
        let slice = std::slice::from_raw_parts(data as *const u8, size);
        let s = std::str::from_utf8_unchecked(slice);
        inner.buffer.push_str(s);
        0
    }
}

/// Represents the ownership state of a `Serializer`.
enum OwnershipState {
    Owned(Box<SerializerInner>),
    #[allow(dead_code)]
    Borrowed(NonNull<spdk_json_write_ctx>),
}

impl OwnershipState {
    /// Returns a pointer to the `spdk_json_write_ctx`.
    fn as_ptr(&self) -> *mut spdk_json_write_ctx {
        match self {
            Self::Owned(inner) => inner.writer.as_ptr(),
            Self::Borrowed(writer) => writer.as_ptr(),
        }
    }

    /// Returns a non-null pointer to the `spdk_json_write_ctx`.
    fn as_non_null_ptr(&self) -> NonNull<spdk_json_write_ctx> {
        match self {
            Self::Owned(inner) => inner.writer,
            Self::Borrowed(writer) => *writer,
        }
    }
}

/// Serializes a Rust data structure into a JSON string.
pub struct Serializer {
    writer: OwnershipState,
}

impl Serializer {
    /// Creates a new `Serializer`.
    fn try_new() -> Result<Self> {
        let writer = OwnershipState::Owned(SerializerInner::try_new()?);

        Ok(Self { writer })
    }

    /// Creates a new `Serializer` from a pointer to a `spdk_json_write_ctx`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the pointer is valid for the lifetime of
    /// the returned instance.
    #[allow(dead_code)]
    pub(crate) unsafe fn from_ptr(writer: *mut spdk_json_write_ctx) -> Self {
        let writer = OwnershipState::Borrowed(NonNull::new_unchecked(writer));

        Self { writer }
    }

    /// Begins writing an object.
    #[inline(always)]
    fn begin_object(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_object_begin(self.writer.as_ptr()) })
    }

    /// Ends writing an object.
    #[inline(always)]
    fn end_object(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_object_end(self.writer.as_ptr()) })
    }

    /// Begins writing an array.
    #[inline(always)]
    fn begin_array(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_array_begin(self.writer.as_ptr()) })
    }

    /// Ends writing an array.
    #[inline(always)]
    fn end_array(&mut self) -> Result<()> {
        to_write_result!(unsafe { spdk_json_write_array_end(self.writer.as_ptr()) })
    }

    /// Writes a name.
    #[inline(always)]
    fn write_name(&mut self, name: &str) -> Result<()> {
        to_write_result!(unsafe {
            spdk_json_write_name_raw(self.writer.as_ptr(), name.as_ptr().cast(), name.len())
        })
    }

    /// Ends writing and returns the JSON string.
    fn end(mut self) -> Result<String> {
        let inner = match &mut self.writer {
            OwnershipState::Owned(inner) => inner,
            OwnershipState::Borrowed(_) => return Err(Error::WriteFailed),
        };

        to_write_result!(unsafe { spdk_json_write_end(inner.writer.as_ptr()) })?;

        Ok(mem::take(&mut inner.buffer))
    }
}

impl ser::Serializer for &mut Serializer {
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
        to_write_result!(unsafe { spdk_json_write_bool(self.writer.as_ptr(), v) })
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
        to_write_result!(unsafe { spdk_json_write_int32(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_i64(self, v: i64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_int64(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u8(self, v: u8) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint8(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u16(self, v: u16) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint16(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u32(self, v: u32) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint32(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_u64(self, v: u64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_uint64(self.writer.as_ptr(), v) })
    }

    #[inline(always)]
    fn serialize_f32(self, v: f32) -> Result<Self::Ok> {
        self.serialize_f64(v as f64)
    }

    #[inline(always)]
    fn serialize_f64(self, v: f64) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_double(self.writer.as_ptr(), v) })
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok> {
        let mut buf = [0; 4];
        let s = v.encode_utf8(&mut buf);

        to_write_result!(unsafe {
            spdk_json_write_string_raw(self.writer.as_ptr(), s.as_ptr().cast(), s.len())
        })
    }

    #[inline(always)]
    fn serialize_str(self, v: &str) -> Result<Self::Ok> {
        to_write_result!(unsafe {
            spdk_json_write_string_raw(self.writer.as_ptr(), v.as_ptr().cast(), v.len())
        })
    }

    #[inline(always)]
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok> {
        to_write_result!(unsafe {
            spdk_json_write_bytearray(self.writer.as_ptr(), v.as_ptr().cast(), v.len())
        })
    }

    #[inline(always)]
    fn serialize_none(self) -> Result<Self::Ok> {
        to_write_result!(unsafe { spdk_json_write_null(self.writer.as_ptr()) })
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

impl ser::SerializeSeq for &mut Serializer {
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

impl ser::SerializeTuple for &mut Serializer {
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

impl ser::SerializeTupleStruct for &mut Serializer {
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

impl ser::SerializeTupleVariant for &mut Serializer {
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

impl ser::SerializeStruct for &mut Serializer {
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

impl ser::SerializeStructVariant for &mut Serializer {
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

impl ser::SerializeMap for &mut Serializer {
    type Ok = ();

    type Error = Error;

    #[inline(always)]
    fn serialize_key<T>(&mut self, key: &T) -> Result<Self::Ok>
    where
        T: ?Sized + ser::Serialize,
    {
        key.serialize(&mut NameSerializer::new(self.writer.as_non_null_ptr()))
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

pub fn to_string<T>(value: &T) -> Result<String>
where
    T: Serialize + ?Sized,
{
    let mut ser = Serializer::try_new()?;
    value.serialize(&mut ser)?;
    ser.end()
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
                .it("should serialize true to true", |_| {
                    expect(to_string(&true)).to_equal(Ok("true".to_string()))
                })
                .it("should serialize false to false", |_| {
                    expect(to_string(&false)).to_equal(Ok("false".to_string()))
                })
                .it("should serialize i8 to a number", |_| {
                    expect(to_string(&42i8)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize i16 to a number", |_| {
                    expect(to_string(&42i16)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize i32 to a number", |_| {
                    expect(to_string(&42i32)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize i64 to a number", |_| {
                    expect(to_string(&42i64)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize u8 to a number", |_| {
                    expect(to_string(&42u8)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize u16 to a number", |_| {
                    expect(to_string(&42u16)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize u32 to a number", |_| {
                    expect(to_string(&42u32)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize u64 to a number", |_| {
                    expect(to_string(&42u64)).to_equal(Ok("42".to_string()))
                })
                .it("should serialize f32 to a number", |_| {
                    expect(to_string(&42.0f32))
                        .to_equal(Ok("4.20000000000000000000e+01".to_string()))
                })
                .it("should serialize f64 to a number", |_| {
                    expect(to_string(&42.0f64))
                        .to_equal(Ok("4.20000000000000000000e+01".to_string()))
                })
                .it("should serialize char to a string", |_| {
                    expect(to_string(&'a')).to_equal(Ok(r#""a""#.to_string()))
                })
                .it("should serialize &str to a string", |_| {
                    expect(to_string("foo")).to_equal(Ok(r#""foo""#.to_string()))
                })
                .it("should serialize &[u8] to an array", |_| {
                    expect(to_string(&[2u8, 3u8, 7u8])).to_equal(Ok("[2,3,7]".to_string()))
                })
                .it("should serialize None to null", |_| {
                    expect(to_string::<Option<i32>>(&None)).to_equal(Ok("null".to_string()))
                })
                .it("should serialize Some(i32) to a number", |_| {
                    expect(to_string(&Some(237i32))).to_equal(Ok("237".to_string()))
                })
                .it("should serialize a sequence to an array", |_| {
                    expect(to_string(&vec![2u16, 3u16, 7u16])).to_equal(Ok("[2,3,7]".to_string()))
                })
                .it("should serialize a tuple to an array", |_| {
                    expect(to_string(&(237, "foo", true)))
                        .to_equal(Ok(r#"[237,"foo",true]"#.to_string()))
                })
                .it("should serialize unit to an empty object", |_| {
                    expect(to_string(&())).to_equal(Ok("{}".to_string()))
                })
                .it("should serialize a unit variant to a string", |_| {
                    expect(to_string(&TestEnum::Unit)).to_equal(Ok(r#""Unit""#.to_string()))
                })
                .it("should serialize a newtype variant to an object", |_| {
                    expect(to_string(&TestEnum::NewType(237)))
                        .to_equal(Ok(r#"{"NewType":237}"#.to_string()))
                })
                .it("should serialize a tuple variant to an object", |_| {
                    expect(to_string(&TestEnum::Tuple(237, "foo", true)))
                        .to_equal(Ok(r#"{"Tuple":[237,"foo",true]}"#.to_string()))
                })
                .it("should serialize a struct variant to an object", |_| {
                    expect(to_string(&TestEnum::Struct {
                        foo: "bar".to_string(),
                        bar: 237,
                    }))
                    .to_equal(Ok(r#"{"Struct":{"foo":"bar","bar":237}}"#.to_string()))
                })
                .it("should serialize a unit struct to an empty object", |_| {
                    expect(to_string(&TestUnitStruct)).to_equal(Ok("{}".to_string()))
                })
                .it("should serialize a newtype struct to a number", |_| {
                    expect(to_string(&TestNewTypeStruct(237))).to_equal(Ok("237".to_string()))
                })
                .it("should serialize a tuple struct to an array", |_| {
                    expect(to_string(&TestTupleStruct(237, "foo", true)))
                        .to_equal(Ok(r#"[237,"foo",true]"#.to_string()))
                })
                .it("should serialize a struct to an object", |_| {
                    expect(to_string(&TestStruct {
                        foo: "bar".to_string(),
                        bar: 237,
                    }))
                    .to_equal(Ok(r#"{"foo":"bar","bar":237}"#.to_string()))
                })
                .it("should serialize a map to an object", |_| {
                    expect(to_string(&BTreeMap::from([
                        ("bar", 42i32),
                        ("foo", 237i32),
                    ])))
                    .to_equal(Ok(r#"{"bar":42,"foo":237}"#.to_string()))
                });
        })
        .state(NullState)
        .run()
    }
}
