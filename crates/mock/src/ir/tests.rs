//! Test the type graph, complete wrapper structure, defaults, and abstract type references.

use std::sync::Arc;

use chigiri::core::Source;
use chigiri::core::language::ast::Value;

use super::{SchemaGraph, Wrapper};

#[test]
fn rsm_01_graph_resolves_cycles_wrappers_defaults_oneof_and_abstract_members() {
    let graph = SchemaGraph::parse(vec![Source::new(
        "schema",
        r#"
        enum Status { OPEN CLOSED }
        interface Node { id: ID! }
        type Item implements Node { id: ID! related: [Item] state: Status! }
        type Payload { item: Item nested: [[String!]!] }
        union Result = Item | Payload
        input Choice @oneOf { id: ID text: String }
        input Filter { state: Status = OPEN recursive: [Filter] }
        type Query { search(filter: Filter = {}, choice: Choice): [Result!]! }
    "#,
    )])
    .unwrap();
    let item = graph.type_id("Item").unwrap();
    let node = graph.type_id("Node").unwrap();
    let payload = graph.type_id("Payload").unwrap();
    assert_eq!(graph.types()[node.0].possible_types, [item]);
    assert_eq!(
        graph.types()[graph.type_id("Result").unwrap().0].possible_types,
        [item, payload]
    );
    let related = &graph.fields()[graph.types()[item.0].fields["related"].0];
    assert_eq!(related.ty.named, item);
    assert_eq!(&*related.ty.wrappers, [Wrapper::List]);
    assert!(graph.types()[graph.type_id("Choice").unwrap().0].one_of);
    let filter = &graph.types()[graph.type_id("Filter").unwrap().0];
    let state = &graph.inputs()[filter.inputs["state"].0];
    assert!(
        matches!(&state.default.as_ref().unwrap().value, Value::Enum(name) if name.value == "OPEN")
    );
    let nested = &graph.fields()[graph.types()[payload.0].fields["nested"].0].ty;
    assert_eq!(
        &*nested.wrappers,
        [
            Wrapper::List,
            Wrapper::NonNull,
            Wrapper::List,
            Wrapper::NonNull
        ]
    );
    assert_eq!(nested.list_depth(), 2);
    assert!(nested.nullable());
    assert!(Arc::ptr_eq(&nested.wrappers, &nested.clone().wrappers));
    let query = &graph.types()[graph.query().0];
    assert_eq!(query.name.as_ref(), "Query");
    assert_eq!(graph.fields()[query.fields["search"].0].arguments.len(), 2);
    assert!(graph.mutation().is_none());
    assert_eq!(
        graph
            .field_source(query.fields["search"])
            .unwrap()
            .name
            .span
            .source_id
            .index(),
        0
    );
}
