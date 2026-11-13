//! Root-coordinate dispatch by position, parent type, and readonly context.

use super::tests::{contract, record, run, script};
use super::*;
use crate::output::ObjectData;
use chigiri::core::Value;

#[test]
fn rsm_08_same_script_uses_typed_parent_and_refined_scope_for_root_and_nested() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type Item { id: ID! name: String! }
        type Query {
            nested: Mutation!
            again: Query!
            scope: String!
        }
        type Mutation { create: Item! }
    "#,
    );
    let mut store = Store::empty(contract.ir().clone());
    let create = script(
        &contract,
        "Mutation.create",
        r#"
if ctx.scope == 'mutation' then
    assert(parent == nil)
    return ctx.models.Item:create({id='Item-1',name='root'})
else
    assert(parent ~= nil)
    return ctx.models.Item:find({limit=1})[1]
end
"#,
    );
    let first = record(run(&create, &Object::new(), &mut store).unwrap());
    let parent = record(
        run(
            &script(&contract, "Query.nested", "return {}"),
            &Object::new(),
            &mut store,
        )
        .unwrap(),
    );
    let nested = record(
        execute_nested(
            &create,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default(),
        )
        .unwrap(),
    );
    let ObjectData::Model(first) = first.data() else {
        panic!("model required")
    };
    let ObjectData::Model(nested) = nested.data() else {
        panic!("model required")
    };
    assert!(Arc::ptr_eq(first, nested));
    assert_eq!(store.snapshot().size().0, 1);
    let scope = script(
        &contract,
        "Query.scope",
        "assert(ctx.scope == 'read') if parent then return 'nested' else return 'root' end",
    );
    assert!(
        matches!(run(&scope, &Object::new(), &mut store).unwrap(), OutputValue::Scalar(Value::String(value)) if value == "root")
    );
    let query_parent = record(
        run(
            &script(&contract, "Query.again", "return {}"),
            &Object::new(),
            &mut store,
        )
        .unwrap(),
    );
    assert!(
        matches!(execute_nested(&scope, &Object::new(), &query_parent, &Cancellation::default(), &Cancellation::default()).unwrap(), OutputValue::Scalar(Value::String(value)) if value == "nested")
    );
    // Forging the visible scope string cannot bypass the read-only table or Rust-side authorization.
    let forged = script(
        &contract,
        "Mutation.create",
        "local c = ctx :: any c.scope = 'mutation' return c.models.Item:create({id='bad',name='bad'})",
    );
    assert!(
        execute_nested(
            &forged,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(store.snapshot().size().0, 1);
}
