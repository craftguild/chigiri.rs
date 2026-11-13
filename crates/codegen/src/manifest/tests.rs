//! Tests the closed manifest grammar, runtime selection, and consumer conflicts without side effects.

use super::{Manifest, RuntimeSource, shape};

const PATH: &str = r#"schema_version = "chigiri/config-v2"
[schema]
directory = "graphql"
[package]
name = "graphql-contract"
version = "0.1.0"
edition = "2024"
[generation]
output = "generated/graphql-contract"
[runtime]
source = "path"
root = "../lab01"
"#;

const CONSUMER: &str = r#"
[consumer]
cargo_manifest = "app/Cargo.toml"
package = "my-app"
generated_dependency = "graphql_contract"
"#;

fn parse(source: &str) -> Result<Manifest, crate::CodegenError> {
    Manifest::parse("chigiri.toml", source)
}

#[test]
fn sg_cli_03_parses_defaults_and_consumer_values() {
    let config = parse(PATH).unwrap();
    assert_eq!(config.schema_directory(), "graphql");
    assert_eq!(config.package().name, "graphql-contract");
    assert_eq!(config.output(), "generated/graphql-contract");
    assert!(config.domain_enums().is_empty());
    assert!(config.consumer().is_none());
    assert_eq!(
        config.runtime(),
        &RuntimeSource::Path {
            root: "../lab01".into()
        }
    );
    let config = parse(&format!("{PATH}{CONSUMER}")).unwrap();
    let consumer = config.consumer().unwrap();
    assert!(consumer.all_targets);
    assert!(!consumer.all_features);
    assert!(!consumer.no_default_features);
    assert!(consumer.features.is_empty());
}

#[test]
fn sg_cli_03_rejects_unknown_duplicate_and_legacy_keys_at_all_levels() {
    for table in [
        "",
        "schema",
        "package",
        "generation",
        "runtime",
        "bindings",
        "consumer",
    ] {
        let source = format!("{PATH}\n[bindings]\ndomain_enums=[]\n{CONSUMER}");
        let mutated = if table.is_empty() {
            format!("unexpected = 1\n{source}")
        } else {
            source.replace(&format!("[{table}]"), &format!("[{table}]\nunexpected = 1"))
        };
        assert!(parse(&mutated).is_err(), "{table}");
    }
    for original in [
        "directory = \"graphql\"",
        "name = \"graphql-contract\"",
        "output = \"generated/graphql-contract\"",
        "source = \"path\"",
    ] {
        assert!(parse(&PATH.replace(original, &format!("{original}\n{original}"))).is_err());
    }
    for source in [
        "version = 1\n[bindings]\ndomain-enums=[]".to_owned(),
        PATH.replace("chigiri/config-v2", "pythia/config-v2"),
        PATH.replace("chigiri/config-v2", "chigiri/config-v1"),
        format!("{PATH}[bindings]\ndomain-enums=[]"),
        format!("{PATH}[bindings]\ndomain_enums=[\"Role\",\"Role\"]"),
        format!("{PATH}[bindings]\ndomain_enums=[\"../Role\"]"),
        format!("{PATH}[bindings]\ndomain_enums=[1]"),
        format!("{PATH}{}", "#".repeat(65536)),
    ] {
        assert!(parse(&source).is_err());
    }
}

#[test]
fn sg_cli_03_rejects_missing_required_fields_and_invalid_cargo_values() {
    for line in PATH.lines().filter(|line| line.contains('=')) {
        assert!(parse(&PATH.replace(line, "")).is_err(), "{line}");
    }
    for (from, to) in [
        ("0.1.0", "0.1"),
        ("0.1.0", "01.2.3"),
        ("2024", "2021"),
        ("graphql-contract", "1invalid"),
    ] {
        assert!(parse(&PATH.replace(from, to)).is_err(), "{to}");
    }
}

#[test]
fn sg_cli_03_validates_consumer_features_against_additive_rules() {
    let source = format!("{PATH}{CONSUMER}");
    for extra in [
        "all_features=true\nfeatures=[\"a\"]",
        "all_features=true\nno_default_features=true",
        "features=[\"a\",\"a\"]",
        "features=[\"a,b\"]",
        "all_targets=\"true\"",
    ] {
        assert!(parse(&format!("{source}{extra}")).is_err(), "{extra}");
    }
    for extra in [
        "features=[\"a\"]\nno_default_features=true",
        "all_features=true",
        "all_targets=false",
    ] {
        parse(&format!("{source}{extra}")).unwrap();
    }
    for line in CONSUMER.lines().filter(|line| line.contains('=')) {
        assert!(parse(&source.replace(line, "")).is_err(), "{line}");
    }
}

#[test]
fn sg_cli_05_runtime_source_and_revision_are_explicit_and_cannot_be_mixed() {
    let git = PATH.replace("source = \"path\"\nroot = \"../lab01\"", "source = \"git\"\nurl = \"https://example.org/team/chigiri.git\"\nrevision = \"bf3a360c185382e1030082ab1a9dbd656aaba9aa\"");
    assert!(matches!(
        parse(&git).unwrap().runtime(),
        RuntimeSource::Git { .. }
    ));
    for source in [
        format!("{git}root=\".\""),
        format!("{PATH}url=\"https://example.org/repo\""),
        PATH.replace("source = \"path\"", "source = \"auto\""),
        format!("{git}branch=\"main\""),
        format!("{git}tag=\"v1\""),
    ] {
        assert!(parse(&source).is_err());
    }
    for revision in [
        "main".to_owned(),
        "unknown".to_owned(),
        "0".repeat(40),
        "A".repeat(40),
        "a".repeat(39),
        format!("{}-dirty", "a".repeat(40)),
    ] {
        assert!(
            parse(&git.replace("bf3a360c185382e1030082ab1a9dbd656aaba9aa", &revision)).is_err()
        );
    }
}

#[test]
fn accepts_unambiguous_repository_urls_without_credentials() {
    for valid in [
        "https://example.org/team/repo.git",
        "https://example.org/team/my%20repo.git",
        "https://example.org/team/repo@v1.git",
        "ssh://git@example.org/team/repo.git",
        "ssh://git@[::1]:2222/team/repo",
    ] {
        assert!(shape::git_url(valid), "{valid}");
    }
    for invalid in [
        "http://example.org/repo",
        "git@example.org:repo",
        "https://token@example.org/repo",
        "ssh://git:secret@example.org/repo",
        "ssh://git%3Asecret@example.org/repo",
        "ssh://git@example.org@evil/repo",
        "https://example.org/repo?token=secret",
        "https://example.org/repo#secret",
        "https://example.org:abc/repo",
        "https://example.org/repo%bad%",
        "https://example.org:65536/repo",
        "https://example.org",
        "https:///repo",
        "https://example.org\n/repo",
    ] {
        assert!(!shape::git_url(invalid), "{invalid}");
    }
}

#[test]
fn sg_cli_05_runtime_only_allows_parent_components_and_rejects_input_output_overlap() {
    for root in [".", "..", "../lab01", "foo/../lab01"] {
        parse(&PATH.replace("../lab01", root)).unwrap();
    }
    for path in [
        "/root",
        "graphql/../elsewhere",
        "./graphql",
        "graphql//a",
        "graphql/",
        "C:/root",
    ] {
        assert!(
            parse(&PATH.replace(
                "directory = \"graphql\"",
                &format!("directory = \"{path}\"")
            ))
            .is_err(),
            "{path}"
        );
    }
    for output in ["graphql", "graphql/generated"] {
        assert!(parse(&PATH.replace("generated/graphql-contract", output)).is_err());
    }
    assert!(
        parse(
            &format!("{PATH}{CONSUMER}")
                .replace("app/Cargo.toml", "generated/graphql-contract/Cargo.toml")
        )
        .is_err()
    );
}

#[test]
fn sg_cli_03_enum_bindings_validate_existence_and_kind_against_the_semantic_schema() {
    let mut sources = chigiri_core::SourceMap::new();
    let id = sources.add(
        "schema.graphql",
        "enum Role { ADMIN USER } type Query { role: Role }",
    );
    let limits = chigiri_core::ExecutionLimits::default();
    let document = chigiri_core::language::parse_type_system_document(
        id,
        sources.get(id).unwrap().text(),
        &limits,
    )
    .document
    .unwrap();
    let schema = chigiri_core::schema::compose_schema(&[document], &limits)
        .schema
        .unwrap();
    for name in ["Query", "String", "Absent"] {
        let manifest = parse(&format!("{PATH}[bindings]\ndomain_enums=[\"{name}\"]")).unwrap();
        let error = manifest.resolve_domain_enums(&schema).unwrap_err();
        assert_eq!(error.report().diagnostics()[0].code(), "SF4012");
        let label = error.report().diagnostics()[0].primary().unwrap();
        assert_eq!(label.path(), "chigiri.toml");
        assert!(label.start() > PATH.len());
    }
    let manifest = parse(&format!("{PATH}[bindings]\ndomain_enums=[\"Role\"]")).unwrap();
    assert!(
        manifest
            .resolve_domain_enums(&schema)
            .unwrap()
            .contains("Role")
    );
}

#[test]
fn diagnostics_do_not_expose_secrets_or_manifest_source_text() {
    let error = parse(&format!(
        "{PATH}url = \"https://token:secret@example.org/private"
    ))
    .unwrap_err();
    assert!(!error.to_string().contains("secret"));
    let diagnostic = &error.report().diagnostics()[0];
    assert!(diagnostic.primary().is_some());
    assert!(!format!("{diagnostic:?}").contains("secret"));
}
