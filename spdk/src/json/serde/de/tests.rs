#![cfg(test)]
use std::collections::BTreeMap;

use laboratory::{LabResult, NullState, describe, expect};

use crate::{
    Uuid,
    json::serde::tests::{
        TestEnum, TestNewTypeStruct, TestStruct, TestTupleStruct, TestUnitStruct,
    },
};

use super::from_str;

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
            .it("should deserialize a string UUID to a Uuid", |_| {
                let test_uuid: Uuid = "87654321-dead-beef-feed-0123456789ab"
                    .parse()
                    .expect("valid UUID");

                expect(from_str(r#""87654321-dead-beef-feed-0123456789ab""#))
                    .to_equal(Ok(test_uuid))
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
