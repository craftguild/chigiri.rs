//! Reject post-validation table mutation across VM conversion and IR return checks.

use super::tests::{args, contract, record, run, script};
use super::*;
use crate::output::ObjectData;
use chigiri::core::Value;

const SDL: &str = r#"
    scalar Data @representation(type: JSON)
    scalar Count @representation(type: INTEGER)
    interface Node { id: ID! }
    type Packet implements Node { id: ID! data: Data child: Packet label: String! @virtual }
    union Result = Packet
    type Query {
        data(value: Data): Data
        count: Count!
        packet: Result!
        node: Node!
        strings: [String]!
    }
"#;

#[test]
fn rsm_07_14_runtime_custom_scalars_abstract_models_and_null_items_are_preserved() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::from_seed_json(
        contract.ir().clone(),
        r#"{"Packet":[{"id":"p","child":{"id":"child"},"data":[null]}]}"#,
    )
    .unwrap();
    let data = script(
        &contract,
        "Query.data",
        "if args.value == nil then return ctx.null end\nreturn args.value",
    );
    assert!(matches!(
        run(&data, &Object::new(), &mut store).unwrap(),
        OutputValue::Null
    ));
    assert!(matches!(
        run(&data, &args(&[("value", Value::Null)]), &mut store).unwrap(),
        OutputValue::Null
    ));
    let body = "return ctx.jsonObject({empty = ctx.jsonArray({}), object = ctx.jsonObject({}), value = ctx.null})";
    let data = script(&contract, "Query.data", body);
    let OutputValue::Scalar(Value::Object(object)) =
        run(&data, &Object::new(), &mut store).unwrap()
    else {
        panic!()
    };
    assert_eq!(object.get("empty"), Some(&Value::List(vec![])));
    assert_eq!(object.get("object"), Some(&Value::Object(Object::new())));
    assert_eq!(object.get("value"), Some(&Value::Null));
    for coordinate in ["Query.packet", "Query.node"] {
        let body = "return ctx.models.Packet:find({where={field='id',op='eq',value='p'}})[1]";
        let parent = record(
            run(
                &script(&contract, coordinate, body),
                &Object::new(),
                &mut store,
            )
            .unwrap(),
        );
        assert!(matches!(parent.data(), ObjectData::Model(_)));
        let nested = script(
            &contract,
            "Packet.label",
            "local child = parent.child if type(child) == 'table' then return child.id end return 'none'",
        );
        let label = execute_nested(
            &nested,
            &Object::new(),
            &parent,
            &Cancellation::default(),
            &Cancellation::default(),
        )
        .unwrap();
        assert!(matches!(label, OutputValue::Scalar(Value::String(value)) if value == "child"));
    }
    let strings = script(&contract, "Query.strings", "return {'x', ctx.null, 'y'}");
    assert!(
        matches!(run(&strings, &Object::new(), &mut store).unwrap(), OutputValue::List(items) if matches!(items.as_slice(), [OutputValue::Scalar(_), OutputValue::Null, OutputValue::Scalar(_)]))
    );
    let count = script(&contract, "Query.count", "return 9007199254740994");
    assert!(matches!(
        run(&count, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Int(9007199254740994))
    ));
}

#[test]
fn rsm_08_14_runtime_rejects_cast_wrong_shape_sparse_arrays_and_host_receiver() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    for (coordinate, body) in [
        ("Query.data", "return nil :: any"),
        ("Query.data", "return {} :: any"),
        ("Query.data", "local t: any = {} t.self = t return t"),
        ("Query.data", "return ctx.jsonObject({[1] = 1} :: any)"),
        (
            "Query.data",
            "pcall(function() ctx.jsonArray(ctx.jsonObject({}) :: any) end) return ctx.null",
        ),
        (
            "Query.data",
            "pcall(function() (ctx.jsonObject :: any)({}, {}) end) return ctx.null",
        ),
        ("Query.count", "return 1.5"),
        ("Query.count", "return 0/0"),
        (
            "Query.packet",
            "return {__typename = 'Missing', id = 'p'} :: any",
        ),
        ("Query.node", "return {id = 'p'} :: any"),
        (
            "Query.packet",
            "return {__typename = 'Packet', id = 'p', label = 'computed'} :: any",
        ),
        ("Query.strings", "return {[1] = 'x', [3] = 'z'} :: any"),
    ] {
        assert!(
            run(
                &script(&contract, coordinate, body),
                &Object::new(),
                &mut store
            )
            .is_err(),
            "{body}"
        );
    }
    let types = contract_with_blog();
    let mut store = Store::empty(types.ir().clone());
    for body in [
        "local find = ctx.models.User.find :: any\npcall(function() find(ctx.models.Post, {}) end)\nreturn {}",
        "local find = ctx.models.User.find :: any\npcall(function() find(ctx.models.User, {}, {}) end)\nreturn {}",
    ] {
        assert!(
            run(
                &script(&types, "Query.getUsers", body),
                &args(&[("limit", Value::Int(20))]),
                &mut store
            )
            .is_err()
        );
    }
}

fn contract_with_blog() -> crate::luau::Contract {
    contract(include_str!("../../fixtures/blog.graphql"))
}
