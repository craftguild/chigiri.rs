//! Exercise generated traits against separate, dependency-free domain sources.

#[path = "../../codegen/tests/support/cargo.rs"]
mod cargo;
#[path = "../../codegen/tests/generation/fixture.rs"]
#[allow(dead_code)]
mod generation_fixture;

use std::fs;
use std::process::Command;

use generation_fixture::{TestDirectory, fixture, read_files};

fn setup() -> TestDirectory {
    let directory = TestDirectory::new("trait-errors");
    let sources = fixture("tests/fixtures/trait-errors");
    fs::create_dir_all(directory.join("schema")).unwrap();
    fs::copy(
        sources.join("schema.graphql"),
        directory.join("schema/schema.graphql"),
    )
    .unwrap();
    directory.generate();
    fs::create_dir_all(directory.join("domain/src")).unwrap();
    fs::copy(
        sources.join("domain.rs"),
        directory.join("domain/src/lib.rs"),
    )
    .unwrap();
    fs::write(
        directory.join("domain/Cargo.toml"),
        "[package]\nname=\"domain\"\nversion=\"0.0.0\"\nedition=\"2024\"\nrust-version=\"1.95\"\n[workspace]\n",
    )
    .unwrap();
    directory.write_consumer(
        "trait-error-consumer",
        &fs::read_to_string(sources.join("consumer.rs")).unwrap(),
    );
    fs::copy(
        sources.join("direct.rs"),
        directory.join("consumer/src/direct.rs"),
    )
    .unwrap();
    let source = directory.join("consumer/src/lib.rs");
    let mut content = fs::read_to_string(&source).unwrap();
    content.push_str("\n#[cfg(test)]\nmod direct;\n");
    fs::write(source, content).unwrap();
    let manifest = directory.join("consumer/Cargo.toml");
    let mut content = fs::read_to_string(&manifest).unwrap();
    content.push_str("domain={path=\"../domain\"}\n");
    fs::write(manifest, content).unwrap();
    directory
}

#[test]
fn ri_01_02_traits_compile_with_external_domain_values_and_errors() {
    let directory = setup();
    let files = read_files(&directory.join("generated"));
    for (path, bytes) in &files {
        assert!(!path.contains("projection"), "{path}");
        if path.ends_with(".rs") {
            let source = std::str::from_utf8(bytes).unwrap();
            for retired in ["ResolverFuture", "project_", "resolve_display_name"] {
                assert!(!source.contains(retired), "{path}: {retired}");
            }
        }
    }
    directory.assert_consumer("test");
    assert_consumer_lanes(&directory);
}

#[test]
fn ri_03_required_error_mapper_and_exact_field_contracts_are_compile_errors() {
    let directory = setup();
    directory.assert_consumer("check");
    let path = directory.join("consumer/src/lib.rs");
    let original = fs::read_to_string(&path).unwrap();
    let cases = [
        (
            "missing mapper",
            "::builder(mapper)",
            "::builder()",
            "argument",
        ),
        (
            "wrong mapper",
            "::builder(mapper)",
            "::builder(|_: std::io::Error| chigiri::FieldError::public(\"wrong\"))",
            "std::io::Error",
        ),
        (
            "different group error",
            "UserResolver<AppContext, Types, AppError>",
            "UserResolver<AppContext, Types, std::io::Error>",
            "AppError",
        ),
        ("wrong parent", "user: &'a User", "user: &'a String", "User"),
        (
            "wrong nullability",
            "Result<Option<User>, AppError>",
            "Result<User, AppError>",
            "Option",
        ),
        (
            "wrong output",
            "Result<String, AppError>",
            "Result<i32, AppError>",
            "String",
        ),
        (
            "missing group",
            ".with_user(UserFields)",
            "",
            "no method named `build`",
        ),
        (
            "missing field",
            "async fn display_name",
            "async fn retired_display_name",
            "not all trait items implemented",
        ),
    ];
    for (label, from, to, diagnostic) in cases {
        assert!(original.contains(from));
        fs::write(&path, original.replace(from, to)).unwrap();
        let output = cargo::run(&mut cargo::command(
            &directory.join("consumer/Cargo.toml"),
            "check",
        ));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{label}: unexpectedly compiled");
        assert!(stderr.contains(diagnostic), "{label}: {stderr}");
        assert!(!stderr.contains("could not execute process"), "{stderr}");
    }
    fs::write(&path, &original).unwrap();
    let schema = directory.join("schema/schema.graphql");
    let source = fs::read_to_string(&schema)
        .unwrap()
        .replace("type User {", "type User { addedField: String!");
    fs::write(&schema, source).unwrap();
    let updated = directory.cli("update");
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let output = cargo::run(&mut cargo::command(
        &directory.join("consumer/Cargo.toml"),
        "check",
    ));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        error.contains("E0046") && error.contains("added_field"),
        "{error}"
    );
    let source = fs::read_to_string(&schema).unwrap() + "\ntype Added { value: String! }\n";
    fs::write(schema, source).unwrap();
    assert!(directory.cli("update").status.success());
    let output = cargo::run(&mut cargo::command(
        &directory.join("consumer/Cargo.toml"),
        "check",
    ));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        error.contains("no method named `build`") && error.contains("Added"),
        "{error}"
    );
}

#[test]
fn ri_01_scaffold_is_rejected_without_writes_and_absent_from_help() {
    let directory = TestDirectory::new("retired-scaffold");
    let before = read_files(&directory.join(""));
    for args in [
        vec!["rust", "scaffold", "--object", "User"],
        vec!["help", "rust", "scaffold"],
    ] {
        let output = cargo::run(
            Command::new(env!("CARGO_BIN_EXE_chigiri"))
                .current_dir(directory.join(""))
                .args(args),
        );
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(read_files(&directory.join("")), before);
    }
    let output = cargo::run(Command::new(env!("CARGO_BIN_EXE_chigiri")).args(["rust", "--help"]));
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("scaffold"));
}

#[test]
fn ri_05_explicit_identity_and_infallible_mappers_compile() {
    let directory = TestDirectory::new("identity-infallible");
    fs::create_dir_all(directory.join("schema")).unwrap();
    fs::write(
        directory.join("schema/schema.graphql"),
        "type Query { value: String! } enum Error { SAFE }",
    )
    .unwrap();
    directory.generate();
    directory.write_consumer("explicit-error-consumer", r#"
struct Types;
impl generated::SchemaTypes for Types {}
struct InfallibleFields;
impl generated::QueryResolver<(), Types, std::convert::Infallible> for InfallibleFields {
    async fn value<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> Result<String, std::convert::Infallible> {
        Ok("ready".to_owned())
    }
}
struct IdentityFields;
impl generated::QueryResolver<(), Types, chigiri::FieldError> for IdentityFields {
    async fn value<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> chigiri::FieldResult<String> {
        Ok("ready".to_owned())
    }
}
pub fn assemble() {
    let _: generated::Resolvers<(), Types> = generated::Resolvers::builder(|e: std::convert::Infallible| match e {})
        .with_query(InfallibleFields).build();
    let _: generated::Resolvers<(), Types> = generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(IdentityFields).build();
}
"#);
    directory.assert_consumer("check");
}

fn assert_consumer_lanes(directory: &TestDirectory) {
    let toolchain = "1.95.0";
    let output = cargo::run(
        Command::new("cargo")
            .arg(format!("+{toolchain}"))
            .args(["test", "--offline", "--locked", "--manifest-path"])
            .arg(directory.join("consumer/Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                fixture("../../target/sibling-generator/fixtures-cache"),
            ),
    );
    assert!(
        output.status.success(),
        "{toolchain}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = cargo::run(
        Command::new("cargo")
            .arg(format!("+{toolchain}"))
            .args([
                "clippy",
                "--offline",
                "--locked",
                "--all-targets",
                "--manifest-path",
            ])
            .arg(directory.join("consumer/Cargo.toml"))
            .args(["--", "-D", "warnings"])
            .env(
                "CARGO_TARGET_DIR",
                fixture("../../target/sibling-generator/fixtures-cache"),
            ),
    );
    assert!(
        output.status.success(),
        "{toolchain} Clippy: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[path = "../../codegen/tests/trait_errors/documentation.rs"]
mod documentation;
