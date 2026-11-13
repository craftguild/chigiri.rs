use chigiri::core::Source;
use serde_json::Value;

use super::{Format, ReferenceDocument, Selection, Topic};
use crate::ir::MockIr;

fn selection(topic: Topic, name: &str) -> Selection {
    Selection::new(Some(topic), Some(name.to_owned())).unwrap()
}

#[test]
fn as_08_models_slots_and_resolvers_share_declared_descriptions() {
    let ir = MockIr::parse(vec![Source::new(
        "schema/001.graphql",
        concat!(
            "\"Account model\" type Account { ",
            "\"Identifier\" id: ID! ",
            "\"\" name: String! ",
            "plain: Boolean ",
            "} ",
            "type Query { \"Lookup resolver\" account(id: ID!): Account }",
        ),
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let full: Value = serde_json::from_slice(&document.render(Format::Json).unwrap()).unwrap();
    let schema = &full["schema"];
    assert_eq!(schema["models"][0]["description"]["text"], "Account model");
    assert_eq!(schema["slots"][0]["description"]["text"], "Identifier");
    assert_eq!(schema["slots"][1]["description"]["text"], "");
    assert!(schema["slots"][2]["description"].is_null());
    assert_eq!(
        schema["resolvers"][0]["description"]["text"],
        "Lookup resolver"
    );

    let model: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Topic::Models, "Account"))
            .unwrap(),
    )
    .unwrap();
    let item = &model["content"]["items"][0];
    assert_eq!(item["description"], "Account model");
    assert_eq!(item["slots"][0]["description"], "Identifier");
    assert_eq!(item["slots"][1]["description"], "");
    assert!(item["slots"][2]["description"].is_null());

    let resolver: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Topic::Resolvers, "Query.account"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        resolver["content"]["items"][0]["description"],
        "Lookup resolver"
    );

    let text = String::from_utf8(document.render(Format::Text).unwrap()).unwrap();
    assert!(text.contains("Account model"));
    assert!(text.contains("Identifier"));
    assert!(text.contains("Lookup resolver"));
}

#[test]
fn as_08_new_description_surfaces_escape_terminal_controls_in_text() {
    let ir = MockIr::parse(vec![Source::new(
        "schema/001.graphql",
        concat!(
            "\"\"\"Model \u{1b}[31m\u{202e}\"\"\" type Account { ",
            "\"\"\"Slot \u{1b}[32m\u{202e}\"\"\" id: ID! } ",
            "type Query { \"\"\"Resolver \u{1b}[33m\u{202e}\"\"\" account: Account }",
        ),
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let text = String::from_utf8(document.render(Format::Text).unwrap()).unwrap();

    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{202e}'));
    for expected in [
        "Model \\u{1b}[31m\\u{202e}",
        "Slot \\u{1b}[32m\\u{202e}",
        "Resolver \\u{1b}[33m\\u{202e}",
    ] {
        assert!(text.contains(expected), "{expected}");
    }
}

#[test]
fn r52_02_synthetic_relation_slots_do_not_inherit_relation_descriptions() {
    let ir = MockIr::parse(vec![Source::new(
        "schema/001.graphql",
        concat!(
            "type User { tenant: ID! code: Int! } ",
            "type Post { id: ID! ",
            "\"\"\"Synthetic relation \\u{1b}[31m\\u{202e}\"\"\" ",
            "author: User @relation(",
            "fields:[\"authorTenant\",\"authorCode\"], ",
            "references:[\"tenant\",\"code\"]) ",
            "\"\"\"Explicit FK \\u{1b}[32m\\u{202e}\"\"\" editorId: ID ",
            "editor: User @relation(fields:[\"editorId\"], references:[\"tenant\"]) ",
            "\"\" blankId: ID ",
            "blank: User @relation(fields:[\"blankId\"], references:[\"tenant\"]) ",
            "absentId: ID ",
            "absent: User @relation(fields:[\"absentId\"], references:[\"tenant\"]) ",
            "} type Query { posts: [Post!]! }",
        ),
    )])
    .unwrap();
    let document = ReferenceDocument::build(Some(&ir)).unwrap();
    let full: Value = serde_json::from_slice(&document.render(Format::Json).unwrap()).unwrap();
    let slots = full["schema"]["slots"].as_array().unwrap();
    let description =
        |name: &str| &slots.iter().find(|slot| slot["name"] == name).unwrap()["description"];
    assert!(description("authorTenant").is_null());
    assert!(description("authorCode").is_null());
    assert_eq!(
        description("editorId")["text"],
        "Explicit FK \\u{1b}[32m\\u{202e}"
    );
    assert_eq!(description("blankId")["text"], "");
    assert!(description("absentId").is_null());

    let model: Value = serde_json::from_slice(
        &document
            .render_selection(Format::Json, &selection(Topic::Models, "Post"))
            .unwrap(),
    )
    .unwrap();
    let slots = model["content"]["items"][0]["slots"].as_array().unwrap();
    let description =
        |name: &str| &slots.iter().find(|slot| slot["name"] == name).unwrap()["description"];
    assert!(description("authorTenant").is_null());
    assert!(description("authorCode").is_null());
    assert_eq!(description("editorId"), "Explicit FK \\u{1b}[32m\\u{202e}");
    assert_eq!(description("blankId"), "");
    assert!(description("absentId").is_null());

    let full_text = String::from_utf8(document.render(Format::Text).unwrap()).unwrap();
    assert_eq!(full_text.matches("Synthetic relation").count(), 1);
    assert_eq!(full_text.matches("Explicit FK").count(), 2);
    assert!(!full_text.contains('\u{1b}'));
    assert!(!full_text.contains('\u{202e}'));
    let model_text = String::from_utf8(
        document
            .render_selection(Format::Text, &selection(Topic::Models, "Post"))
            .unwrap(),
    )
    .unwrap();
    assert!(!model_text.contains("Synthetic relation"));
    assert!(model_text.contains("Explicit FK \\u{1b}[32m\\u{202e}"));
    assert_eq!(model_text.matches("description:").count(), 2);
}
