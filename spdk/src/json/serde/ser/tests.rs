#![cfg(test)]
use std::collections::BTreeMap;

use laboratory::{LabResult, NullState, describe, expect};

use crate::json::serde::tests::{
    TestEnum, TestNewTypeStruct, TestStruct, TestTupleStruct, TestUnitStruct,
};

use super::to_string;

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
                expect(to_string(&42.0f32)).to_equal(Ok("4.20000000000000000000e+01".to_string()))
            })
            .it("should serialize f64 to a number", |_| {
                expect(to_string(&42.0f64)).to_equal(Ok("4.20000000000000000000e+01".to_string()))
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
