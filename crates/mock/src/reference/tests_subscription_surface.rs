//! Delivery metadata remains outside the public source graph and schema reference.

use std::collections::BTreeMap;

use chigiri::core::Source;
use serde_json::Value;

use super::{Format, ReferenceDocument, Selection, Topic};
use crate::ir::MockIr;

const PUBLIC: &str = concat!(
    "type Event { id: ID! value: Int! }\n",
    "type Query { event: Event }\n",
    "type Subscription { changed(topic: String!): Event! }\n",
);

#[test]
fn delivery_metadata_does_not_inject_public_schema_or_reference_fields() {
    let ir = MockIr::parse(vec![Source::new("schema.graphql", PUBLIC)]).unwrap();
    assert_eq!(ir.graph().source().sources()[0].text(), PUBLIC);
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let reference: Value = serde_json::from_slice(&document.render(Format::Json).unwrap()).unwrap();
    let schema = &reference["schema"];
    let objects: BTreeMap<_, _> = schema["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|ty| ty["kind"] == "object")
        .map(|ty| (ty["id"].as_str().unwrap(), ty["name"].as_str().unwrap()))
        .collect();
    let mut names: Vec<_> = objects.values().copied().collect();
    names.sort_unstable();
    assert_eq!(names, ["Event", "Query", "Subscription"]);
    let mut coordinates: Vec<_> = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            let parent = objects[field["parent_type_id"].as_str().unwrap()];
            assert_eq!(field["visibility"], "public");
            format!("{parent}.{}", field["name"].as_str().unwrap())
        })
        .collect();
    coordinates.sort_unstable();
    assert_eq!(
        coordinates,
        [
            "Event.id",
            "Event.value",
            "Query.event",
            "Subscription.changed"
        ]
    );

    for (name, expected) in [
        ("Event", vec!["id", "value"]),
        ("Query", vec!["event"]),
        ("Subscription", vec!["changed"]),
    ] {
        let selection = Selection::new(Some(Topic::Types), Some(name.to_owned())).unwrap();
        let topic: Value =
            serde_json::from_slice(&document.render_selection(Format::Json, &selection).unwrap())
                .unwrap();
        let fields: Vec<_> = topic["content"]["items"][0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect();
        assert_eq!(fields, expected, "{name}");
    }
    // Exact field sets catch any injected loss/internal field, not only one spelling.
    let inputs: Vec<_> = schema["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| input["name"].as_str().unwrap())
        .collect();
    assert_eq!(inputs, ["topic"]);
}
