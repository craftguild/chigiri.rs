//! Directive-topic tests for rules, safe text, and related commands.

use super::{Format, ReferenceDocument, Selection, Topic, topic_types::TopicDocument};
use crate::ir::MockIr;
use chigiri::core::Source;
use serde_json::{Value, json};

const BIDI_CONTROLS: [char; 12] = [
    '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

fn selection(topic: Option<Topic>, name: Option<&str>) -> Selection {
    Selection::new(topic, name.map(str::to_owned)).unwrap()
}

#[test]
fn ref_10_directive_topics_inline_every_related_rule_and_reject_open_rule_values() {
    let document = ReferenceDocument::build(None).unwrap();
    let bytes = document
        .render_selection(Format::Json, &selection(Some(Topic::Directives), None))
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let items = value["content"]["items"].as_array().unwrap();
    for item in items {
        let ids = item["rule_ids"].as_array().unwrap();
        let rules = item["rules"].as_array().unwrap();
        assert_eq!(ids.len(), rules.len(), "{}", item["name"]);
        for id in ids {
            let rule = rules.iter().find(|rule| rule["id"] == *id).unwrap();
            assert!(
                rule["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
            assert!(rule["failure_class"].is_string());
        }
    }
    let constraint = items
        .iter()
        .find(|item| item["name"] == "constraint")
        .unwrap();
    assert_eq!(constraint["rules"].as_array().unwrap().len(), 3);

    let mut open = value;
    open["content"]["items"][6]["rules"][0]
        .as_object_mut()
        .unwrap()
        .insert("unknown".into(), json!(true));
    assert!(serde_json::from_value::<TopicDocument>(open).is_err());
}

#[test]
fn ref_14_human_text_escapes_bidi_in_directive_defaults_and_keeps_json_value() {
    let default: String = BIDI_CONTROLS.into_iter().collect();
    let sdl = format!(
        "directive @audit(note: String = \"{default}\") on FIELD_DEFINITION\n\
         type Query {{ value: String }}"
    );
    let ir = MockIr::parse(vec![Source::new("bidi.graphql", sdl)]).unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    for chosen in [
        selection(Some(Topic::Directives), Some("audit")),
        selection(Some(Topic::All), None),
    ] {
        let text =
            String::from_utf8(document.render_selection(Format::Text, &chosen).unwrap()).unwrap();
        for ch in BIDI_CONTROLS {
            assert!(!text.contains(ch), "U+{:04X} was born.", ch as u32);
            assert!(
                text.contains(&format!("\\u{{{:x}}}", ch as u32)),
                "U+{:04X} is not visible.",
                ch as u32
            );
        }
    }
    let json: Value = serde_json::from_slice(
        &document
            .render_selection(
                Format::Json,
                &selection(Some(Topic::Directives), Some("audit")),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        json["content"]["items"][0]["arguments"][0]["default"],
        json!({"kind": "string", "value": default})
    );
}

#[test]
fn ref_14_every_directive_related_command_resolves_to_a_closed_selection() {
    let document = ReferenceDocument::build(None).unwrap();
    let value: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Some(Topic::Directives), None))
            .unwrap(),
    )
    .unwrap();
    for item in value["content"]["items"].as_array().unwrap() {
        for related in item["related"].as_array().unwrap() {
            let mut parts = related.as_str().unwrap().split_whitespace();
            let topic = Topic::parse(parts.next().unwrap()).unwrap();
            let selected = Selection::new(topic.into(), parts.next().map(str::to_owned)).unwrap();
            assert!(parts.next().is_none() && document.accepts(&selected));
        }
    }
}
