//! Build raw fixtures inside one VM, then wrap them with the product-owned value API.

use super::{Codec, VmValue};
use crate::ir::{MockIr, TypeRef};
use crate::value::ValueError;
use chigiri::core::{Source, Value};
use luaur::{Lua, Value as LuaValue};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub(super) struct TestCodec(pub Codec);
impl Deref for TestCodec {
    type Target = Codec;
    fn deref(&self) -> &Codec {
        &self.0
    }
}
impl DerefMut for TestCodec {
    fn deref_mut(&mut self) -> &mut Codec {
        &mut self.0
    }
}

pub(super) trait FixtureValue {
    fn bound(&self, codec: &Codec) -> VmValue;
}
impl FixtureValue for LuaValue {
    fn bound(&self, codec: &Codec) -> VmValue {
        codec.wrap_for_test(self.clone())
    }
}
impl FixtureValue for VmValue {
    fn bound(&self, _: &Codec) -> VmValue {
        self.clone()
    }
}
impl TestCodec {
    pub fn null(&self) -> LuaValue {
        self.0.null_lua()
    }
    pub fn decode(
        &mut self,
        ir: &MockIr,
        ty: &TypeRef,
        value: &impl FixtureValue,
    ) -> Result<Value, ValueError> {
        self.0.decode(ir, ty, &value.bound(&self.0))
    }
    pub fn decode_json(
        &mut self,
        ir: &MockIr,
        value: &impl FixtureValue,
    ) -> Result<Value, ValueError> {
        self.0.decode_json(ir, &value.bound(&self.0))
    }
}

pub(super) const SDL: &str = r#"
scalar Json @representation(type: JSON)
scalar Map @representation(type: OBJECT)
scalar Array @representation(type: ARRAY)
scalar Integer @representation(type: INTEGER)
input Input { optional: String, list: [Json] }
type Device {
 id: ID!
 name: String!
 data: Json
 owner: Device @relation(fields:["ownerId"],references:["id"])
 computed: Int @virtual
}
type Packet { device: Device, data: Json }
union Result = Device | Packet
type Query {
 json: Json
 map: Map
 array: Array
 list: [Json]
 integer: Integer
 devices: [Device!]!
 result: Result
 read(input: Input): Packet
}
"#;

pub(super) fn ir() -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("codec.graphql", SDL)]).unwrap())
}
pub(super) fn ty<'a>(ir: &'a MockIr, field: &str) -> &'a TypeRef {
    let field = ir.graph().types()[ir.graph().query().0].fields[field];
    &ir.graph().fields()[field.0].ty
}
pub(super) fn setup() -> (Lua, TestCodec, Arc<MockIr>) {
    let lua = Lua::new();
    let codec = Codec::new(&lua).unwrap();
    (lua, TestCodec(codec), ir())
}
