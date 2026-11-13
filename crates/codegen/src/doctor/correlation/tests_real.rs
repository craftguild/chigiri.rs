//! Correlate actual rustc return-position future bounds, including source paths.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::{DiagnosticPolicy, ProcessLimits, ResolvedManifest, run_bounded_process};

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn diag_09_real_cargo_future_output_is_correlated_with_its_trait() {
    let repository =
        fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let directory = Directory(
        repository
            .join("target/sibling-generator")
            .join(format!("real-return-bound-{}", std::process::id())),
    );
    let root = &directory.0;
    fs::create_dir_all(root.join("graphql")).unwrap();
    fs::create_dir_all(root.join("consumer/src")).unwrap();
    fs::write(
        root.join("graphql/schema.graphql"),
        "type Query { user: User } type User { id: ID! }",
    )
    .unwrap();
    fs::write(
        root.join("chigiri.toml"),
        concat!(
            "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n",
            "[package]\nname=\"real-return-contract\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
            "[generation]\noutput=\"generated\"\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n",
        ),
    )
    .unwrap();
    let manifest = ResolvedManifest::load(&root.join("chigiri.toml")).unwrap();
    crate::generate_contract(&manifest, DiagnosticPolicy::default()).unwrap();
    fs::write(root.join("consumer/Cargo.toml"), format!(
        "[package]\nname=\"return-bound-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\ncontract={{package=\"real-return-contract\",path=\"../generated\"}}\nchigiri={{path={:?}}}\n",
        repository.join("crates/facade").to_str().unwrap(),
    )).unwrap();
    fs::write(root.join("consumer/src/lib.rs"), concat!(
        "pub struct Types; pub struct User;\n",
        "impl ::contract::SchemaTypes for Types { type User = User; }\n",
        "pub struct Fields;\n",
        "impl ::contract::UserResolver<(), Types, chigiri::FieldError> for Fields {\n",
        "async fn id<'a>(&'a self, _: chigiri::FieldContext<'a, ()>, _: &'a User) -> chigiri::FieldResult<String> { Ok(String::new()) }\n}\n",
    )).unwrap();
    let output = run_bounded_process(
        Command::new(env!("CARGO"))
            .current_dir(root.join("consumer"))
            .args(["check", "--offline", "--message-format=json"])
            .env(
                "CARGO_TARGET_DIR",
                repository.join("target/sibling-generator/fixtures-cache"),
            ),
        &[],
        ProcessLimits {
            timeout: Duration::from_secs(60),
            bytes: 1024 * 1024,
        },
    )
    .unwrap();
    assert!(!output.status.success());
    let schema = crate::inspect_contract_schema(&manifest).unwrap();
    let sources = super::Sources::new(
        manifest.root.clone(),
        manifest.root.relative("consumer").unwrap(),
        manifest.root.relative("generated").unwrap(),
        "contract",
    );
    let record = output
        .stdout
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find(|record| {
            record
                .pointer("/message/code/code")
                .and_then(|v| v.as_str())
                == Some("E0271")
        })
        .unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    let message = &record["message"];
    let primary = super::primary_span(message).unwrap();
    let object = schema.object("User").unwrap();
    assert!(
        super::qualified::method(primary, object, "id", &sources),
        "primary: {record}"
    );
    assert!(
        super::has_generated_span(message, &sources),
        "generated: {record}"
    );
    assert_eq!(
        super::supplements(&record, &schema, &sources).len(),
        1,
        "{record}"
    );
}
