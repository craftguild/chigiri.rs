//! Embedded interface/union values remain typed values, not records or relations.

use chigiri::core::{Object, Value};

use super::tests::{contract, record, run, script};
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;

const SDL: &str = r#"
    interface Node { label: String! }
    type TextNode implements Node { label: String! rendered: String! @virtual }
    type NumberNode implements Node { label: String! count: Int! }
    union Content = TextNode | NumberNode
    type Document { nodes: [Node!]! content: Content }
    type Query { documents: [Document!]! }
    type Mutation { make: Document! }
"#;

fn seed(source: &str) -> Object {
    chigiri_http::value_from_json(serde_json::from_str(source).unwrap())
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn sd_embedded_abstract_seed_requires_possible_typename_without_registering_children() {
    let contract = contract(SDL);
    let ir = contract.ir();
    let store = Store::seeded(ir.clone(), &seed(r#"{"Document":[{"nodes":[{"__typename":"TextNode","label":"text"},{"__typename":"NumberNode","label":"number","count":3}],"content":null}]}"#)).unwrap();
    assert_eq!(store.snapshot().size().0, 1);
    for name in ["TextNode", "NumberNode"] {
        let model = ir.model_for(ir.graph().type_id(name).unwrap()).unwrap();
        assert_eq!(store.snapshot().count(model, &Object::new()).unwrap(), 0);
    }
    for node in [
        r#"{"label":"missing"}"#,
        r#"{"__typename":"Document","label":"wrong"}"#,
        r#"{"__typename":"NumberNode","label":"missing count"}"#,
        r#"{"__typename":"TextNode","label":"computed","rendered":"bad"}"#,
        r#"{"__typename":"NumberNode","label":"wrong count","count":"3"}"#,
    ] {
        assert!(
            Store::seeded(
                ir.clone(),
                &seed(&format!(r#"{{"Document":[{{"nodes":[{node}]}}]}}"#))
            )
            .is_err(),
            "{node}"
        );
    }
}

#[test]
fn sd_embedded_vm_preserves_concrete_types_and_virtual_parent_values() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let create = script(
        &contract,
        "Mutation.make",
        r#"
        return ctx.models.Document:create({
            nodes = {{__typename = "TextNode", label = "local"}},
            content = {__typename = "NumberNode", label = "count", count = 3},
        })
    "#,
    );
    let virtual_field = script(&contract, "TextNode.rendered", "return parent.label");
    let mut store = Store::empty(contract.ir().clone());
    let parent = record(run(&create, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(row) = parent.data() else {
        panic!("document record required")
    };
    let ir = contract.ir();
    let doc_type = ir.graph().type_id("Document").unwrap();
    let field = ir.graph().types()[doc_type.0].fields["nodes"];
    let model = ir.model_for(doc_type).unwrap();
    let slot = ir.models()[model.0].slots["nodes"];
    let mut reader = parent.snapshot().reader();
    let OutputValue::List(nodes) = reader.output(field, row.get(slot).unwrap()).unwrap() else {
        panic!("list required")
    };
    let OutputValue::Object(child) = &nodes[0] else {
        panic!("concrete object required")
    };
    assert_eq!(child.ty(), ir.graph().type_id("TextNode").unwrap());
    assert!(matches!(child.data(), ObjectData::Dto(_)));
    let rendered = super::execute_nested(
        &virtual_field,
        &Object::new(),
        child,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(rendered, OutputValue::Scalar(Value::String(value)) if value == "local"));
    assert_eq!(store.snapshot().size().0, 1);
}
