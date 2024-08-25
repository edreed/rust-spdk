use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub(crate) enum TestEnum {
    Unit,
    NewType(i32),
    Tuple(i32, &'static str, bool),
    Struct{ foo: String, bar: i32 },
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestUnitStruct;

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestNewTypeStruct(pub(crate) i32);

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestTupleStruct(pub(crate) i32, pub(crate) &'static str, pub(crate) bool);

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub(crate) struct TestStruct {
    pub(crate) foo: String,
    pub(crate) bar: i32,
}

