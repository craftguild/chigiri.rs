use std::collections::BTreeMap;

use chigiri::core::Source;
use serde_json::json;

use super::super::{ConsumerRegistration, EventSettings, EventSettingsError};
use crate::ir::MockIr;

fn registration(enabled: bool) -> ConsumerRegistration {
    ConsumerRegistration {
        field: "changed".into(),
        enabled,
    }
}

fn settings(entries: impl IntoIterator<Item = (String, ConsumerRegistration)>) -> EventSettings {
    EventSettings::new(Default::default(), entries.into_iter().collect()).unwrap()
}

#[test]
fn ev_05_registry_accepts_bounded_identifiers_and_retains_disabled_entries() {
    let names = ["_".into(), "Audit_09".into(), "x".repeat(64)];
    let settings = settings(
        names
            .iter()
            .cloned()
            .map(|name| (name, registration(false))),
    );
    assert_eq!(settings.consumers().len(), 3);
    let bytes = serde_json::to_vec(&settings).unwrap();
    assert_eq!(
        serde_json::from_slice::<EventSettings>(&bytes).unwrap(),
        settings
    );
}

#[test]
fn ev_05_rejects_path_escapes_unicode_reserved_shapes_and_case_collisions() {
    for name in [
        "",
        "0audit",
        "../audit",
        "a/b",
        "a\\b",
        ".",
        "..",
        "audit.luau",
        "a-b",
        "a b",
        "a\0b",
        "caf\u{e9}",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(["a".repeat(65)])
    {
        let entries = BTreeMap::from([(name.clone(), registration(true))]);
        assert_eq!(
            EventSettings::new(Default::default(), entries).unwrap_err(),
            EventSettingsError::ConsumerName,
            "{name:?}"
        );
        assert!(
            serde_json::from_value::<EventSettings>(json!({
                "consumers": {name: {"field": "changed", "enabled": true}},
            }))
            .is_err()
        );
    }
    let entries = BTreeMap::from([
        ("audit".into(), registration(true)),
        ("Audit".into(), registration(false)),
    ]);
    assert_eq!(
        EventSettings::new(Default::default(), entries).unwrap_err(),
        EventSettingsError::ConsumerCollision
    );
    for source in [
        r#"{"consumers":{"audit":{"field":"changed","enabled":true},"Audit":{"field":"changed","enabled":false}}}"#,
        r#"{"consumers":{"audit":{"field":"changed","enabled":true},"audit":{"field":"changed","enabled":false}}}"#,
    ] {
        assert!(serde_json::from_str::<EventSettings>(source).is_err());
    }
}

#[test]
fn ev_05_registry_capacity_includes_disabled_entries() {
    for count in [1_024, 1_025] {
        let entries: BTreeMap<_, _> = (0..count)
            .map(|index| (format!("consumer_{index}"), registration(false)))
            .collect();
        let value = json!({"consumers": entries});
        assert_eq!(
            serde_json::from_value::<EventSettings>(value).is_ok(),
            count == 1_024
        );
        assert_eq!(
            EventSettings::new(Default::default(), entries).is_ok(),
            count == 1_024
        );
    }
}

#[test]
fn ev_05_registration_requires_boolean_enabled_and_field_without_extra_keys() {
    for entry in [
        json!({"field": "changed"}),
        json!({"enabled": false}),
        json!({"field": "changed", "enabled": 1}),
        json!({"field": "changed", "enabled": null}),
        json!({"field": "changed", "enabled": "true"}),
        json!({"field": 4, "enabled": false}),
        json!({"field": "changed", "enabled": false, "path": "other.luau"}),
    ] {
        assert!(
            serde_json::from_value::<EventSettings>(json!({"consumers": {"audit": entry}}))
                .is_err()
        );
    }
}

#[test]
fn ev_05_resolves_actual_adopted_custom_root_and_only_its_fields() {
    let ir = MockIr::parse_with_storage(
        vec![Source::new(
            "public.graphql",
            r#"
            schema { query: Read subscription: Changes }
            type Read { changed: String! }
            type Changes { changed(required: ID!): [String]! }
        "#,
        )],
        vec![Source::new("storage.graphql", "type Private { id: ID! }")],
    )
    .unwrap();
    let configured = settings([("audit".into(), registration(false))]);
    let resolved = configured.resolve(&ir).unwrap();
    assert_eq!(resolved.len(), 1);
    let consumer = &resolved[0];
    assert_eq!(consumer.name(), "audit");
    assert!(!consumer.enabled());
    assert_eq!(consumer.path().as_str(), "consumers/audit.luau");
    let field = &ir.graph().fields()[consumer.field().0];
    assert_eq!(field.name.as_ref(), "changed");
    assert_eq!(ir.graph().types()[field.parent.0].name.as_ref(), "Changes");

    for field in ["Read.changed", "Changes.changed", "missing", "Changed", ""] {
        let invalid = settings([(
            "audit".into(),
            ConsumerRegistration {
                field: field.into(),
                enabled: false,
            },
        )]);
        assert_eq!(
            invalid.resolve(&ir).unwrap_err(),
            EventSettingsError::ConsumerField
        );
    }
    let no_subscription = MockIr::parse(vec![Source::new(
        "public.graphql",
        "type Query { changed: String! }",
    )])
    .unwrap();
    assert_eq!(
        configured.resolve(&no_subscription).unwrap_err(),
        EventSettingsError::ConsumerField
    );
    assert!(
        EventSettings::default()
            .resolve(&no_subscription)
            .unwrap()
            .is_empty()
    );
}
