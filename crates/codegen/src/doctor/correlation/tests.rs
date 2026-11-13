use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{ResolvedManifest, inspect_contract_schema};
use serde_json::{Value, json};

use super::supplements;

static NEXT: AtomicU64 = AtomicU64::new(0);

#[path = "tests_scope.rs"]
mod scope;

#[test]
fn diag_08_nested_error_generics_use_only_the_outer_registration_index() {
    for text in [
        "ResolverStage<(), Types, AppError<Inner, 0>, 2>",
        "ResolverStage<Arc<Context>, Types, AppError<fn() -> Option<Inner>>, 2>",
    ] {
        assert_eq!(super::stage_index(text, "ResolverStage"), Some(2));
    }
    for text in [
        "ResolverStage<(), Types, AppError<{ 1 > 0 }>, 2>",
        "ResolverStage<(), Types, AppError<'>'>, 2>",
        "ResolverStage<(), Types, AppError<Inner>, -1>",
    ] {
        assert_eq!(super::stage_index(text, "ResolverStage"), None);
    }
}

struct Fixture {
    root: PathBuf,
    schema: crate::DeveloperSchema,
    sources: super::Sources,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "chigiri-correlation-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("graphql")).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(
            root.join("graphql/schema.graphql"),
            concat!(
                "type User { id: ID!, name: String! }\n",
                "type Team { id: ID! }\n",
                "type Query { greeting: String! }\n",
            ),
        )
        .unwrap();
        fs::create_dir_all(root.join("generated/__chigiri")).unwrap();
        fs::write(
            root.join("generated/__chigiri/stage.rs"),
            "Production stage",
        )
        .unwrap();
        fs::write(root.join("generated/__chigiri/resolver.rs"), "fn greeting").unwrap();
        let root = fs::canonicalize(root).unwrap();
        fs::write(root.join("chigiri.toml"), "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n[package]\nname=\"correlation\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[generation]\noutput=\"generated\"\n[runtime]\nsource=\"path\"\nroot=\".\"\n").unwrap();
        let manifest = ResolvedManifest::load(&root.join("chigiri.toml")).unwrap();
        let schema = inspect_contract_schema(&manifest).unwrap();
        let sources = super::Sources::new(
            manifest.root.clone(),
            manifest.root.clone(),
            manifest.root.relative("generated").unwrap(),
            "generated",
        );
        Self {
            root,
            schema,
            sources,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn diag_08_reports_missing_group_with_context() {
    let fixture = Fixture::new();
    let group_source = "::generated::Resolvers::builder().build();";
    fs::write(fixture.root.join("src/lib.rs"), group_source).unwrap();
    let mut group = record(
        "E0599",
        "no method named `build` for ResolverStage<(), Types, chigiri::FieldError, 0>",
        group_source,
        "ResolverStage",
    );
    let start = group_source.rfind("build").unwrap();
    group["message"]["spans"][0]["byte_start"] = start.into();
    group["message"]["spans"][0]["byte_end"] = (start + 5).into();
    let hints = supplements(&group, &fixture.schema, &fixture.sources);
    assert_eq!(hints[0].code(), "SF4018");
    assert!(hints[0].help()[0].contains(".with_user(...)"));
}

#[test]
fn diag_08_reports_the_exact_missing_root_trait_field() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("src/lib.rs"),
        "impl generated::QueryResolver<(), Types, chigiri::FieldError> for Root {}",
    )
    .unwrap();
    let record = json!({
        "reason": "compiler-message",
        "message": {
            "code": {"code": "E0046"},
            "message": "not all trait items implemented",
            "spans": [
                span("src/lib.rs", true, "impl generated::QueryResolver<(), Types, chigiri::FieldError> for Root {}", "missing greeting"),
                span("generated/__chigiri/resolver.rs", false, "fn greeting<'a>(", "`greeting` from trait")
            ]
        }
    });
    let hints = supplements(&record, &fixture.schema, &fixture.sources);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].message().contains("Query.greeting"));
    assert!(hints[0].help()[0].contains("QueryResolver"));
}

#[test]
fn diag_09_trait_output_shows_graphql_and_rust_types() {
    let fixture = Fixture::new();
    let source = "impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields {\n    async fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() }\n}";
    fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    fs::write(
        fixture.root.join("generated/__chigiri/resolver.rs"),
        "fn id<'a>(",
    )
    .unwrap();
    let start = source.find("async fn").unwrap();
    let end = source.find(" { todo!").unwrap();
    let record = json!({"message": {
        "code": {"code": "E0271"}, "message": "expected future to be a future that resolves to `Result<Id, FieldError>`, but it resolves to `Result<String, FieldError>`",
        "spans": [
            {"file_name":"src/lib.rs", "is_primary":true,
             "byte_start":start, "byte_end":end, "line_start":2, "column_start":5},
            span("generated/__chigiri/resolver.rs", false, "fn id<'a>(", "`id` from trait")
        ]
    }});
    let hints = supplements(&record, &fixture.schema, &fixture.sources);
    assert_eq!(hints[0].code(), "SF4019");
    assert!(hints[0].help()[0].contains("GraphQL type `ID!`"));
    assert!(hints[0].help()[0].contains("Rust type `::chigiri::Id`"));
}

#[test]
fn diag_09_return_bound_span_identifies_the_trait_method() {
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("consumer/src")).unwrap();
    let prefix = include_str!("../../../tests/sibling_cli/doctor_consumer.rs");
    let mut source = prefix[..prefix.find("pub fn resolvers").unwrap()].replace(
        "SchemaTypes for Types {}",
        "SchemaTypes for Types { type User = User; }",
    );
    source.push_str(concat!(
        "\npub struct User { pub id: ::chigiri::Id, pub name: String }\n",
        "pub struct UserFields;\n",
        "impl ::contract::UserResolver<(), Types, ::chigiri::FieldError> for UserFields {\n",
        "async fn id<'a>(&'a self, _: ::chigiri::FieldContext<'a, ()>, user: &'a User) -> ::chigiri::FieldResult<String> { Ok(user.id.to_string()) }\n",
        "async fn name<'a>(&'a self, _: ::chigiri::FieldContext<'a, ()>, user: &'a User) -> ::chigiri::FieldResult<String> { Ok(user.name.clone()) }\n}\n",
    ));
    fs::write(fixture.root.join("consumer/src/lib.rs"), &source).unwrap();
    fs::write(
        fixture.root.join("generated/__chigiri/resolver.rs"),
        "Output = Result<Id, E>,",
    )
    .unwrap();
    let manifest = ResolvedManifest::load(&fixture.root.join("chigiri.toml")).unwrap();
    fixture.sources = super::Sources::new(
        manifest.root.clone(),
        manifest.root.relative("consumer").unwrap(),
        manifest.root.relative("generated").unwrap(),
        "contract",
    );
    let start = source
        .find("::chigiri::FieldResult<String> { Ok(user.id")
        .unwrap();
    let mut primary = span("src/lib.rs", true, "return type", "expected Id");
    primary["byte_start"] = start.into();
    primary["byte_end"] = (start + "::chigiri::FieldResult<String>".len()).into();
    primary["line_start"] = 20.into();
    let object = fixture.schema.object("User").unwrap();
    assert!(super::qualified::method(
        &primary,
        object,
        "id",
        &fixture.sources
    ));
    let record = json!({"message": {
        "code": {"code":"E0271"}, "message":"expected future to be a future that resolves to `Result<Id, FieldError>`, but it resolves to `Result<String, FieldError>`",
        "spans": [primary, span("../generated/__chigiri/resolver.rs", false, "Output = Result<Id, E>,", "required by this bound in `UserResolver::id::{anon_assoc#0}`")]
    }});
    let hints = supplements(&record, &fixture.schema, &fixture.sources);
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].code(), "SF4019");
}

#[test]
fn diag_10_requires_more_than_a_matching_compiler_message() {
    let fixture = Fixture::new();
    let mut record = record(
        "E0599",
        "no method named build for ResolverStage<(), Types, chigiri::FieldError, 0>",
        "generated::Resolvers::<(), Types>::builder().build();",
        "ordinary source",
    );
    record["message"]["spans"].as_array_mut().unwrap().pop();
    assert!(supplements(&record, &fixture.schema, &fixture.sources).is_empty());

    let ordinary = json!({
        "reason": "compiler-message",
        "message": {
            "code": {"code": "E0308"},
            "message": "mismatched types",
            "spans": [span("src/lib.rs", true, "let value: u32 = \"wrong\";", "expected u32")]
        }
    });
    assert!(supplements(&ordinary, &fixture.schema, &fixture.sources).is_empty());
}

#[test]
fn diag_10_rejects_invalid_utf8_byte_offsets() {
    let fixture = Fixture::new();
    let source = "é generated::Resolvers::builder().build();";
    fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    let mut record = record(
        "E0599",
        "no method named `build` for ResolverStage<(), Types, chigiri::FieldError, 0>",
        source,
        "ResolverStage",
    );
    record["message"]["spans"][0]["byte_start"] = 1.into();
    assert!(supplements(&record, &fixture.schema, &fixture.sources).is_empty());
}

#[test]
fn diagonal_10_builder_omits_internal_unrelated_errors_and_standalone_text() {
    for (code, text, source, selected) in [
        (
            "E0599",
            "no method named build for ResolverStage<(), Types, chigiri::FieldError, 0>",
            "let old = ::generated::Resolvers::builder(); unrelated.build();",
            "build",
        ),
        (
            "E0599",
            "no method named build for user::Builder<(), Types, 0>",
            "let text = \"::generated::projection::User::<(), Types>::builder()\"; unrelated.build();",
            "build",
        ),
        (
            "E0599",
            "no method named build for ResolverStage<(), Types, chigiri::FieldError, 0>",
            "ordinary::generated::Resolvers::builder().build();",
            "build",
        ),
        (
            "E0308",
            "mismatched types",
            "::generated::projection::User::<(), Types>::builder().project_id(|_| { let unrelated: u32 = \"wrong\"; todo!() });",
            "\"wrong\"",
        ),
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
        let mut record = record(code, text, source, "generated symbol");
        let start = source.rfind(selected).unwrap();
        record["message"]["spans"][0]["byte_start"] = start.into();
        record["message"]["spans"][0]["byte_end"] = (start + selected.len()).into();
        assert!(
            supplements(&record, &fixture.schema, &fixture.sources).is_empty(),
            "{source}"
        );
    }
}

#[test]
fn diag_10_requires_a_generated_span_that_identifies_the_external_trait() {
    for source in [
        "impl ordinary::QueryResolver<(), Types, chigiri::FieldError> for Root {}",
        "impl /* ::generated::QueryResolver */ Ordinary for Root {}",
        "extern crate self as generated; impl ::generated::QueryResolver<(), Types, chigiri::FieldError> for Root {}",
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
        let record = json!({"message": {"code":{"code":"E0046"},"message":"missing `greeting`", "spans":[span("src/lib.rs",true,source,"consumer")]}});
        assert!(
            supplements(&record, &fixture.schema, &fixture.sources).is_empty(),
            "{source}"
        );
    }
}

fn record(code: &str, message: &str, source: &str, generated: &str) -> Value {
    json!({
        "reason": "compiler-message",
        "message": {
            "code": {"code": code},
            "message": message,
            "spans": [
                span("src/lib.rs", true, source, "consumer"),
                span("generated/__chigiri/stage.rs", false, generated, "generated symbol")
            ]
        }
    })
}

fn span(file: &str, primary: bool, text: &str, label: &str) -> Value {
    json!({
        "file_name": file,
        "is_primary": primary,
        "byte_start": 0,
        "byte_end": text.len(),
        "line_start": 1,
        "column_start": 1,
        "label": label,
        "text": [{"text": text}]
    })
}
