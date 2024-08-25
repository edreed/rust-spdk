#![cfg(test)]
use std::collections::BTreeMap;

use laboratory::{
    LabResult,
    NullState,

    describe,
    expect,
};
use serde::{
    Deserialize,
    Serialize,
};

use super::{
    de::from_str,
    ser::to_string,
};

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub(crate) enum TestEnum<'a> {
    Unit,
    NewType(i32),
    Tuple(i32, &'a str, bool),
    Struct{ foo: String, bar: i32 },
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestUnitStruct;

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestNewTypeStruct(pub(crate) i32);

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestTupleStruct<'a>(pub(crate) i32, pub(crate) &'a str, pub(crate) bool);

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestStruct {
    pub(crate) foo: String,
    pub(crate) bar: i32,
}

#[test]
fn suite() -> LabResult {
    describe("to_string()", |suite| {
        suite

        .it("should serialize and deserialize true", |_| {
            let bool_true = true;
            expect(from_str(to_string(&bool_true).unwrap().as_ref()))
                .to_equal(Ok(bool_true))
        })

        .it("should serialize and deserialize false", |_| {
            let bool_false = false;
            expect(from_str(to_string(&bool_false).unwrap().as_ref()))
                .to_equal(Ok(bool_false))
        })

        .it("should serialize and deserialze an i8", |_| {
            let i8_val = 42i8;
            expect(from_str(to_string(&i8_val).unwrap().as_ref()))
                .to_equal(Ok(i8_val))
        })

        .it("should serialize and deserialize an i16", |_| {
            let i16_val = 42i16;
            expect(from_str(to_string(&i16_val).unwrap().as_ref()))
                .to_equal(Ok(i16_val))
        })

        .it("should serialize and deserialize an i32", |_| {
            let i32_val = 42i32;
            expect(from_str(to_string(&i32_val).unwrap().as_ref()))
                .to_equal(Ok(i32_val))
        })

        .it("should serialize and deserialize an i64", |_| {
            let i64_val = 42i64;
            expect(from_str(to_string(&i64_val).unwrap().as_ref()))
                .to_equal(Ok(i64_val))
        })

        .it("should serialize and deserialize an u8", |_| {
            let u8_val = 42u8;
            expect(from_str(to_string(&u8_val).unwrap().as_ref()))
                .to_equal(Ok(u8_val))
        })

        .it("should serialize and deserialize a u16", |_| {
            let u16_val = 42u16;
            expect(from_str(to_string(&u16_val).unwrap().as_ref()))
                .to_equal(Ok(u16_val))
        })

        .it("should serialize and deserialize a u32", |_| {
            let u32_val = 42u32;
            expect(from_str(to_string(&u32_val).unwrap().as_ref()))
                .to_equal(Ok(u32_val))
        })

        .it("should serialize and deserialize a u64", |_| {
            let u64_val = 42u64;
            expect(from_str(to_string(&u64_val).unwrap().as_ref()))
                .to_equal(Ok(u64_val))
        })

        .it("should serialize and deserialize an f32", |_| {
            let f32_val = 42.0f32;
            expect(from_str(to_string(&f32_val).unwrap().as_ref()))
                .to_equal(Ok(f32_val))
        })

        .it("should serialize and deserialize an f64", |_| {
            let f64_val = 42.0f64;
            expect(from_str(to_string(&f64_val).unwrap().as_ref()))
                .to_equal(Ok(f64_val))
        })

        .it("should serialize and deserialize a char", |_| {
            let char_val = 'a';
            expect(from_str(to_string(&char_val).unwrap().as_ref()))
                .to_equal(Ok(char_val))
        })

        .it("should serialize and deserialize a &str to a string", |_| {
            let str_val = "foo";
            expect(from_str(to_string(str_val).unwrap().as_ref()))
                .to_equal(Ok(str_val.to_string()))
        })

        .it("should serialize and deserialize [u8]", |_| {
            let array_val = [2u8, 3u8, 7u8];
            expect(from_str(to_string(&array_val).unwrap().as_ref()))
                .to_equal(Ok(array_val))
        })

        .it("should serialize and deserialize None", |_| {
            let none_val: Option<i32> = None;
            expect(from_str(to_string(&none_val).unwrap().as_ref()))
                .to_equal(Ok(none_val))
        })

        .it("should serialize and deserialize Some(i32)", |_| {
            let some_val = Some(237i32);
            expect(from_str(to_string(&some_val).unwrap().as_ref()))
                .to_equal(Ok(some_val))
        })

        .it("should serialize and deserialize a vector", |_| {
            let vec_value = vec![2u16, 3u16, 7u16];
            expect(from_str(to_string(&vec_value).unwrap().as_ref()))
                .to_equal(Ok(vec_value))
        })

        .it("should serialize and deserialize a tuple", |_| {
            let tuple_val = (237, "foo", true);
            expect(from_str(to_string(&tuple_val).unwrap().as_ref()))
                .to_equal(Ok(tuple_val))
        })

        .it("should serialize and desierialize a unit", |_| {
            let unit_val = ();
            expect(from_str(to_string(&unit_val).unwrap().as_ref()))
                .to_equal(Ok(unit_val))
        })

        .it("should serialize and deserialize a unit variant", |_| {
            let unit_variant = TestEnum::Unit;
            expect(from_str(to_string(&unit_variant).unwrap().as_ref()))
                .to_equal(Ok(unit_variant))
        })

        .it("should serialize and deserialize a newtype variant", |_| {
            let new_type = TestEnum::NewType(237);
            expect(from_str(to_string(&new_type).unwrap().as_ref()))
                .to_equal(Ok(new_type))
        })

        .it("should serialize and deserialize a tuple variant", |_| {
            let tuple_variant = TestEnum::Tuple(237, "foo", true);
            expect(from_str(to_string(&tuple_variant).unwrap().as_ref()))
                .to_equal(Ok(tuple_variant))
        })

        .it("should serialize a struct variant to an object", |_| {
            let struct_variant = TestEnum::Struct{ foo: "bar".to_string(), bar: 237 };
            expect(from_str(to_string(&struct_variant).unwrap().as_ref()))
                .to_equal(Ok(struct_variant))
        })

        .it("should serialize and deserialzie a unit struct", |_| {
            let unit_struct = TestUnitStruct;
            expect(from_str(to_string(&unit_struct).unwrap().as_ref()))
                .to_equal(Ok(unit_struct))
        })

        .it("should serialize and deserialize a newtype struct", |_| {
            let new_type_struct = TestNewTypeStruct(237);
            expect(from_str(to_string(&new_type_struct).unwrap().as_ref()))
                .to_equal(Ok(new_type_struct))
        })

        .it("should serialize and deserialize a tuple struct", |_| {
            let tuple_struct = TestTupleStruct(237, "foo", true);
            expect(from_str(to_string(&tuple_struct).unwrap().as_ref()))
                .to_equal(Ok(tuple_struct))
        })

        .it("should serialize and deserialize a struct", |_| {
            let struct_val = TestStruct{ foo: "bar".to_string(), bar: 237 };
            expect(from_str(to_string(&struct_val).unwrap().as_ref()))
                .to_equal(Ok(struct_val))
        })

        .it("should serialize and deserialize a map", |_| {
            let map = BTreeMap::from([("bar", 42i32), ("foo", 237i32)]);
            expect(from_str(to_string(&map).unwrap().as_ref()))
                .to_equal(Ok(map))
        });
    }).state(NullState).run()
}
