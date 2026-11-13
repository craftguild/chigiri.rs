use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::{Source, Value};
use luaur::{Lua, Value as LuaValue};

use super::Codec;
use crate::ir::MockIr;
use crate::output::OutputValue;
use crate::store::Store;

#[test]
fn captured_parent_encoding_keeps_private_slots_without_current_row_identity() {
    let _case = crate::vm_gate::test_case();
    let _guard = crate::vm_gate::test_guard();
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "captured-parent.graphql",
            r#"
            type User { id: ID! posts: [Post!]! @relation }
            type Post {
              id: ID!
              author: User! @relation(fields:["authorId"],references:["id"])
            }
            type Query { posts: [Post!]! }
            type Subscription { changed: Post! }
            "#,
        )])
        .unwrap(),
    );
    let ty = ir.graph().type_id("Post").unwrap();
    let model = ir.model_for(ty).unwrap();
    let slots = &ir.models()[model.0].slots;
    let values = BTreeMap::from([
        (slots["id"], Value::from("p")),
        (slots["authorId"], Value::from("u")),
    ]);
    let snapshot = Store::empty(ir).snapshot();
    let OutputValue::Object(parent) =
        crate::output::captured_record(snapshot, ty, model, values, Some("p".into())).unwrap()
    else {
        panic!("object required")
    };
    let lua = Lua::new();
    let mut codec = Codec::new(&lua).unwrap();
    let encoded = codec.encode_parent(&parent).unwrap();
    let LuaValue::Table(table) = encoded.as_lua() else {
        panic!("table required")
    };
    let typename: String = table.raw_get("__typename").unwrap();
    let author: String = table.raw_get("authorId").unwrap();
    assert_eq!(typename, "Post");
    assert_eq!(author, "u");
    let ir = parent.snapshot().ir();
    let root = ir.graph().subscription().unwrap();
    let field = ir.graph().types()[root.0].fields["changed"];
    let (value, provenance) = codec
        .decode_output(ir, &ir.graph().fields()[field.0].ty, &encoded)
        .unwrap();
    let output = crate::output::validate_provenance_detailed(
        parent.snapshot(),
        field,
        &value,
        &provenance,
        &mut crate::value::ValueBudget::default(),
    )
    .unwrap();
    assert!(
        matches!(output, OutputValue::Object(object) if matches!(object.data(), crate::output::ObjectData::CapturedRecord(_)))
    );
}
