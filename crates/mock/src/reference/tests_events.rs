//! Discover exact type selectors and configured consumers without executing callbacks.

use std::collections::BTreeMap;

use chigiri::core::Source;
use serde_json::{Value, json};

use super::{Format, InputKind, ReferenceDocument, Selection, Topic};
use crate::events::{ConsumerRegistration, EventSettings, SourceLimits};
use crate::ir::MockIr;

const SDL: &str = r#"
schema {query:Read mutation:Write subscription:Signals}
type Item {id:ID! title:String!}
type Other {id:ID! title:String!}
type QueryOnly {id:ID!}
type Read {a:Item q:QueryOnly}
type Write {emit:Int!}
type Signals {
  "A captured item."
  b(required:ID!):Item!
  c:Item
  list:[Item!]! @constraint(minItems:2)
  other:Other!
}
"#;

fn ir() -> MockIr {
    MockIr::parse(vec![Source::new("events.graphql", SDL)]).unwrap()
}

fn configured(kind: InputKind) -> ReferenceDocument {
    let ir = ir();
    let events = EventSettings::new(
        SourceLimits::default(),
        BTreeMap::from([
            (
                "active".into(),
                ConsumerRegistration {
                    field: "b".into(),
                    enabled: true,
                },
            ),
            (
                "disabled".into(),
                ConsumerRegistration {
                    field: "list".into(),
                    enabled: false,
                },
            ),
        ]),
    )
    .unwrap();
    ReferenceDocument::from_configuration(
        kind,
        &ir,
        &Default::default(),
        &Default::default(),
        &events,
    )
    .unwrap()
}

fn topic(doc: &ReferenceDocument, kind: Topic, name: &str) -> Value {
    let selection = Selection::new(Some(kind), Some(name.into())).unwrap();
    serde_json::from_slice(&doc.render_selection(Format::Json, &selection).unwrap()).unwrap()
}

#[test]
fn ev_01_05_named_type_reference_uses_generated_shapes_and_destination_owned_sources() {
    let ir = ir();
    let doc = ReferenceDocument::build(Some(&ir)).unwrap();
    let wire: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
    assert_eq!(wire["events"]["configured"], false);
    assert_eq!(wire["events"]["consumers"], json!([]));
    assert_eq!(wire["events"]["types"].as_array().unwrap().len(), 2);
    let item = wire["events"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "Item")
        .unwrap();
    assert_eq!(item["destinations"].as_array().unwrap().len(), 3);
    assert_eq!(
        item["publish_signature"],
        "ctx.pubsub.publish.Item(input: ChigiriItemEventPublishInput) -> ()"
    );
    let input = item["publish_input_definition"].as_str().unwrap();
    assert!(
        input.contains("ChigiriItemRecord | ChigiriItemRecord | ChigiriNull | {ChigiriItemRecord}")
    );
    let b = item["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["coordinate"] == "Signals.b")
        .unwrap();
    assert_eq!(b["source_type"], "ChigiriSignalsBSource");
    assert_eq!(b["description"], "A captured item.");
    assert_eq!(b["payload_type"], "ChigiriItemRecord");
    assert!(
        b["consumer_subscribe_signature"]
            .as_str()
            .unwrap()
            .contains("subscribe(ctx: ChigiriSignalsBConsumerSetupContext)")
    );
    assert!(
        !b["consumer_subscribe_signature"]
            .as_str()
            .unwrap()
            .contains("args")
    );
    let runtime = topic(&doc, Topic::Runtime, "pubsub");
    let encoded = serde_json::to_string(&runtime).unwrap();
    assert!(encoded.contains("publish.Item") && encoded.contains("subscribe.Item"));
    assert!(!encoded.contains("publish.b(") && !encoded.contains("subscribe.b("));
    assert!(!encoded.contains("publish.QueryOnly"));
    let generator = topic(&doc, Topic::Runtime, "generator");
    let encoded = serde_json::to_string(&generator).unwrap();
    assert!(
        encoded.contains("sequence.Item") && encoded.contains("ChigiriSignalsListSequenceInput")
    );
    let signatures = generator["content"]["items"][0]["signatures"]
        .as_array()
        .unwrap();
    assert!(signatures.iter().any(|v| {
        v.as_str()
            .unwrap()
            .contains("[\"values\"]: {ChigiriItemRecord}")
    }));
    assert!(signatures.iter().any(|v| {
        v.as_str()
            .unwrap()
            .contains("[\"values\"]: {{ChigiriItemRecord}}")
    }));
}

#[test]
fn ev_05_consumer_reference_preserves_disabled_configuration_and_authority_without_readiness() {
    for kind in [InputKind::Project, InputKind::Bundle] {
        let doc = configured(kind);
        let all: Value = serde_json::from_slice(&doc.render(Format::Json).unwrap()).unwrap();
        assert_eq!(all["events"]["configured"], true);
        assert_eq!(all["config_executed"], kind == InputKind::Project);
        let named = topic(&doc, Topic::Consumers, "disabled");
        assert_eq!(named["readiness_checked"], false);
        assert_eq!(named["content"]["items"].as_array().unwrap().len(), 1);
        let disabled = &named["content"]["items"][0];
        assert_eq!(disabled["enabled"], false);
        assert_eq!(disabled["module_path"], "consumers/disabled.luau");
        assert_eq!(disabled["source"]["coordinate"], "Signals.list");
        assert_eq!(disabled["source"]["payload_type"], "{ChigiriItemRecord}");
        assert_eq!(disabled["source"]["constraints"]["min_items"], 2);
        let closed: super::topic_types::TopicDocument =
            serde_json::from_value(named.clone()).unwrap();
        assert_eq!(serde_json::to_value(closed).unwrap(), named);
        let mut forged = named;
        forged["content"]["items"][0]["secret"] = json!(true);
        assert!(serde_json::from_value::<super::topic_types::TopicDocument>(forged).is_err());
        let selection = Selection::new(Some(Topic::Consumers), Some("active".into())).unwrap();
        let text =
            String::from_utf8(doc.render_selection(Format::Text, &selection).unwrap()).unwrap();
        assert!(
            text.contains("SIGNATURES")
                && text.contains("AUTHORITY")
                && text.contains("ctx.request is ctx.null")
        );
        assert!(text.contains("runtime readiness were not checked"));
        assert!(
            !doc.accepts(&Selection::new(Some(Topic::Consumers), Some("missing".into())).unwrap())
        );
    }
}

#[test]
fn ev_01_field_reference_identifies_subscription_directory_and_read_only_setup() {
    let doc = ReferenceDocument::build(Some(&ir())).unwrap();
    let reference = topic(&doc, Topic::Resolvers, "Signals.b");
    let resolver = &reference["content"]["items"][0];
    assert_eq!(resolver["module_path"], "resolvers/subscription/b.luau");
    assert_eq!(resolver["scopes"][0]["scope"], "subscription_setup");
    assert_eq!(
        resolver["subscription_source"]["source_type"],
        "ChigiriSignalsBSource"
    );
    assert!(
        resolver["scopes"][0]["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["methods"] == json!(["count", "find"]))
    );
    assert!(
        topic(&doc, Topic::Resolvers, "Write.emit")["content"]["items"][0]["subscription_source"]
            .is_null()
    );
}
