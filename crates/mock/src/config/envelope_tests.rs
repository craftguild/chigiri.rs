//! Verify that reference loading skips serve readiness but still rejects malformed closed envelopes.

use super::{Config, Envelope};
use std::path::Path;
use toml_edit::{DocumentMut, value};

const TEMPLATE: &str = include_str!("../../fixtures/mock-v11-ready.toml");
const LEGACY_BUNDLE: &str = include_str!("../../fixtures/mock-v9-ready.toml");
const OLD_VOCABULARY: &str = include_str!("../../fixtures/mock-v6.toml");

fn mixed_generation_bundles() -> [String; 2] {
    let current_with_legacy_capability = TEMPLATE.replacen(
        "chigiri-mock-capabilities/v4",
        "chigiri-mock-capabilities/v2",
        1,
    );
    let legacy_with_current_capability = LEGACY_BUNDLE.replacen(
        "chigiri-mock-capabilities/v2",
        "chigiri-mock-capabilities/v4",
        1,
    );
    assert_ne!(current_with_legacy_capability, TEMPLATE);
    assert_ne!(legacy_with_current_capability, LEGACY_BUNDLE);
    let current = current_with_legacy_capability
        .parse::<DocumentMut>()
        .unwrap();
    let legacy = legacy_with_current_capability
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(current["format"].as_str(), Some("chigiri-mock/v11"));
    assert_eq!(
        current["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v2")
    );
    assert_eq!(legacy["format"].as_str(), Some("chigiri-mock/v9"));
    assert_eq!(
        legacy["capabilities"].as_str(),
        Some("chigiri-mock-capabilities/v4")
    );
    [
        current_with_legacy_capability,
        legacy_with_current_capability,
    ]
}

fn portable_with_storage(document: DocumentMut, storage: &str) -> String {
    let source = document.to_string().replace(
        "[clock]",
        &format!(
            "[[storage.sources]]\nname = \"storage.graphql\"\ntext = '''\n{storage}\n'''\n\n[clock]"
        ),
    );
    let mut document = source.parse::<DocumentMut>().unwrap();
    let entries = document["schema"]["sources"].as_array_of_tables().unwrap();
    let public: Vec<_> = entries
        .iter()
        .take(entries.len() - 1)
        .map(|entry| {
            chigiri::core::Source::new(
                entry["name"].as_str().unwrap().to_owned(),
                entry["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let public_digest = chigiri_mock::ir::SchemaGraph::parse(public.clone())
        .unwrap()
        .source()
        .digest()
        .to_hex();
    let mut combined = public;
    combined.push(chigiri::core::Source::new("storage.graphql", storage));
    let storage_digest = chigiri_mock::ir::SchemaGraph::parse(combined)
        .unwrap()
        .source()
        .digest()
        .to_hex();
    document["schema"]["digest"] = value(public_digest);
    document["storage"]["digest"] = value(storage_digest);
    document.to_string()
}

#[test]
fn mr_18_envelope_accepts_invalid_luau_and_seed_without_full_validation() {
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["resolvers"]["Query.getUsers"]["luau"] = value("private invalid Lua(");
    document["seed"]["records_json"] = value("private malformed JSON");
    let source = document.to_string();
    let envelope = Envelope::parse(Path::new("reference.toml"), &source).unwrap();
    assert_eq!(envelope.ir.models().len(), 2);
    assert!(Config::parse(Path::new("serve.toml"), &source).is_err());
}

#[test]
fn mr_18_envelope_does_not_apply_serve_listen_or_origin_readiness() {
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["server"]["listen"] = value("0.0.0.0:4000");
    document["server"]["allowed_origins"] = value(toml_edit::Array::from_iter(["*"]));
    let source = document.to_string();
    assert!(Envelope::parse(Path::new("reference.toml"), &source).is_ok());
    assert!(Config::parse(Path::new("serve.toml"), &source).is_err());
}

#[test]
fn tl_02_portable_clock_uses_integer_unix_seconds_and_rejects_text() {
    let _test_case = crate::vm_gate::test_case();
    let path = Path::new("reference.toml");
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["clock"]["mode"] = value("fixed");
    document["clock"]["now"] = value(1789776000_i64);
    let envelope = Envelope::parse(path, &document.to_string()).unwrap();
    assert_eq!(envelope.clock.fixed_value(), Some(1789776000));

    document["clock"]["now"] = value("2026-09-19T00:00:00.000Z");
    assert!(Envelope::parse(path, &document.to_string()).is_err());
}

#[test]
fn mr_19_envelope_rejects_old_unknown_malformed_digest_and_coordinate_mismatch() {
    let path = Path::new("reference.toml");
    assert!(Envelope::parse(path, OLD_VOCABULARY).is_err());
    assert!(Config::parse(path, OLD_VOCABULARY).is_err());
    assert!(Envelope::parse(path, include_str!("../../fixtures/mock-v10-ready.toml")).is_err());
    assert!(Config::parse(path, include_str!("../../fixtures/mock-v10-ready.toml")).is_err());
    assert!(Envelope::parse(path, LEGACY_BUNDLE).is_err());
    assert!(Config::parse(path, LEGACY_BUNDLE).is_err());
    for source in [
        TEMPLATE.replace("chigiri-mock/v11", "pythia-mock/v8"),
        TEMPLATE.replace(
            "chigiri-mock-capabilities/v4",
            "pythia-mock-capabilities/v2",
        ),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v1"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v4"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v6"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v7"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v8"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v9"),
        TEMPLATE.replace("chigiri-mock/v11", "chigiri-mock/v10"),
        TEMPLATE.replace(
            "chigiri-mock-capabilities/v4",
            "chigiri-mock-capabilities/v0",
        ),
        TEMPLATE.replace(
            "chigiri-mock-capabilities/v4",
            "chigiri-mock-capabilities/v1",
        ),
        TEMPLATE.replace(
            "chigiri-mock-capabilities/v4",
            "chigiri-mock-capabilities/v3",
        ),
        format!("unknown = 1\n{TEMPLATE}"),
        TEMPLATE.replace("[server]", "[server"),
        TEMPLATE.replace("[server]", "[server]\nunknown = true"),
    ] {
        assert!(Envelope::parse(path, &source).is_err());
    }
    let mut legacy = TEMPLATE.parse::<DocumentMut>().unwrap();
    legacy["resolvers"]["Query.getUsers"]["state"] = value("ready");
    assert!(Envelope::parse(path, &legacy.to_string()).is_err());
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["schema"]["digest"] = value("0".repeat(64));
    assert!(Envelope::parse(path, &document.to_string()).is_err());
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["resolvers"]
        .as_table_mut()
        .unwrap()
        .remove("Query.getUsers");
    assert!(Envelope::parse(path, &document.to_string()).is_err());
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    document["resolvers"]["unknown"] = document["resolvers"]["Query.getUsers"].clone();
    assert!(Envelope::parse(path, &document.to_string()).is_err());
}

#[test]
fn current_reader_rejects_both_bundle_capability_generation_mixes() {
    let path = Path::new("mixed.toml");
    Envelope::parse(path, TEMPLATE).unwrap();
    Config::parse(path, TEMPLATE).unwrap();
    for source in mixed_generation_bundles() {
        assert!(Envelope::parse(path, &source).is_err());
        assert!(Config::parse(path, &source).is_err());
    }
}

#[test]
fn sa_12_storage_schema_failure_keeps_its_bundle_toml_position() {
    let source = TEMPLATE.replace(
        "[clock]",
        "[[storage.sources]]\nname = \"storage.graphql\"\ntext = '''\ntype Broken {\n'''\n\n[clock]",
    );
    let document = source.parse::<toml_edit::ImDocument<String>>().unwrap();
    let start = document["storage"]["sources"]
        .as_array_of_tables()
        .unwrap()
        .get(0)
        .unwrap()["text"]
        .span()
        .unwrap()
        .start;
    let error = Envelope::parse(Path::new("portable.toml"), &source)
        .err()
        .unwrap();
    assert_eq!(error.toml, crate::diagnostic::position(&source, start));
}

#[test]
fn sa_06_portable_storage_cannot_replace_public_operation_roots() {
    let root = "schema { query: Mutation mutation: Query }";
    let source = portable_with_storage(TEMPLATE.parse().unwrap(), root);
    let error = Envelope::parse(Path::new("portable.toml"), &source)
        .err()
        .unwrap();
    assert_eq!(error.message, "storage operation root is not allowed");
}

#[test]
fn sa_06_portable_storage_rejects_public_enum_extensions() {
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    let public = document["schema"]["sources"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    document["schema"]["sources"][0]["text"] =
        value(format!("{public}\nenum PublicStatus {{ PUBLIC }}"));
    let source = portable_with_storage(document, "extend enum PublicStatus { STORAGE_ONLY }");
    let error = Envelope::parse(Path::new("portable.toml"), &source)
        .err()
        .unwrap();
    assert_eq!(error.message, "unsupported storage declaration");
}

#[test]
fn sa_17_current_bundle_reconstructs_request_state_and_enabled_hook_without_external_files() {
    let mut document = TEMPLATE.parse::<DocumentMut>().unwrap();
    let state =
        r#"{"fields":{"authenticated":{"kind":"boolean"}},"initial":{"authenticated":false}}"#;
    document["state"]["contract_json"] = value(state);
    document["hooks"]["request"]["before"] = value(true);
    let source = "return {run=function(ctx) if ctx.state.authenticated then ctx.reject({code=401}) end return nil end}";
    document["hooks"]["modules"]["request.before"] =
        toml_edit::Item::Table(toml_edit::Table::new());
    document["hooks"]["modules"]["request.before"]["luau"] = value(source);
    let config = Config::parse(
        Path::new("nonexistent-portable.toml"),
        &document.to_string(),
    )
    .unwrap();
    assert_eq!(config.checked.hooks().len(), 1);
    assert_eq!(config.checked.hooks()[0].source(), source);
    assert_eq!(
        config
            .checked
            .scripts()
            .contract()
            .state()
            .initial()
            .get("authenticated"),
        Some(&chigiri::core::Value::Boolean(false))
    );
    for edit in ["missing", "disabled", "extra", "bad-state", "wrong-hook"] {
        let mut changed = document.clone();
        match edit {
            "missing" => {
                changed["hooks"]["modules"]
                    .as_table_mut()
                    .unwrap()
                    .remove("request.before");
            }
            "disabled" => changed["hooks"]["request"]["before"] = value(false),
            "extra" => changed["hooks"]["unknown"] = value(true),
            "bad-state" => {
                changed["state"]["contract_json"] =
                    value("{\"fields\":{},\"initial\":{\"unknown\":true}}")
            }
            _ => {
                changed["hooks"]["modules"]["request.before"]["luau"] =
                    value("return {run=function() return 1 end}")
            }
        }
        assert!(Config::parse(Path::new("bad.toml"), &changed.to_string()).is_err());
    }
}
