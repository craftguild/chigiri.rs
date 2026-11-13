//! Preserve generated visibility, drift, semantic layout, and resource coverage.

use std::fs;

#[path = "../../codegen/tests/support/cargo.rs"]
mod cargo;
#[path = "../../codegen/tests/trait_surface/fixture.rs"]
mod fixture;
#[path = "../../codegen/tests/trait_surface/rustdoc.rs"]
mod rustdoc;

use fixture::{TestDirectory, generated_files, manifest_root};

#[test]
fn ri_01_08_private_adapters_and_assembly_stages_are_hidden_from_rustdoc() {
    let root = fs::read_to_string(manifest_root().join("tests/golden/full/src/lib.rs")).unwrap();
    assert!(root.contains("#[doc(hidden)]\nmod __chigiri;"));
    assert!(root.contains("pub use __chigiri::*;"));
    let bridge = fs::read_to_string(
        manifest_root().join("tests/golden/full/src/__chigiri/internal/bridge/dispatch.rs"),
    )
    .unwrap();
    assert!(bridge.contains("#[doc(hidden)]\nimpl<C, T>"));

    let temporary = TestDirectory::new("rustdoc");
    let manifest = manifest_root().join("tests/compile/pass/Cargo.toml");
    let output = cargo::run(
        cargo::command(&manifest, "doc")
            .args(["--no-deps", "--package", "chigiri-codegen-full-contract"])
            .env("CARGO_TARGET_DIR", temporary.join("target"))
            .env("RUSTDOCFLAGS", "-D warnings"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let documentation = temporary.join("target/doc/chigiri_codegen_full_contract");
    let user = fs::read_to_string(documentation.join("trait.UserResolver.html")).unwrap();
    for required in ["updated_at", "FieldContext", "Result"] {
        assert!(user.contains(required), "{required}");
    }
    assert!(!documentation.join("projection").exists());
    let resolvers = fs::read_to_string(documentation.join("struct.Resolvers.html")).unwrap();
    for hidden in ["ResolverDispatch", "schema_fingerprint", "ArgumentValues"] {
        assert!(!resolvers.contains(hidden), "rustdoc leaked {hidden}");
    }
    let search = rustdoc::rustdoc_search_index(&temporary.join("target/doc"));
    for hidden in [
        "ResolverDispatch",
        "ResolverStage",
        "UserAdapter",
        "ProjectionPlan",
    ] {
        assert!(!search.contains(hidden), "rustdoc search leaked {hidden}");
    }
}

#[test]
fn ri_07_detects_private_dispatch_drift_without_writes() {
    let temporary = TestDirectory::new("drift");
    temporary.generate(&manifest_root().join("tests/fixtures/full"));
    let output = temporary.join("generated");
    let shard = output.join("src/__chigiri/internal/dispatch/user.rs");
    let mut source = fs::read_to_string(&shard).unwrap();
    source.push_str("// Change used to verify drift detection.\n");
    fs::write(&shard, source).unwrap();
    let before = generated_files(&output);
    let error = chigiri_codegen::check_contract(&temporary.manifest()).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4020");
    assert!(
        error
            .message()
            .contains("changed: src/__chigiri/internal/dispatch/user.rs")
    );
    assert!(error.message().contains("different name"));
    assert!(error.message().contains("chigiri rust codegen"));
    assert_eq!(generated_files(&output), before);
}

#[test]
fn ri_07_large_generated_semantic_units_keep_resource_bounds() {
    let temporary = TestDirectory::new("large");
    temporary.generate(&manifest_root().join("../../tests/fixtures/vnext-large-product/graphql"));
    let files = generated_files(&temporary.join("generated"));
    assert!(files.values().map(Vec::len).sum::<usize>() <= 64 * 1024 * 1024);
    assert!(files.len() <= 65_536);
    assert!(files.contains_key("src/__chigiri/resolvers/query.rs"));
    assert!(files.contains_key("src/__chigiri/resolvers/mutation.rs"));
    assert!(!files.keys().any(|name| name.contains("projection")));
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/resolvers/objects/"))
            .count(),
        69
    );
    assert_eq!(
        files
            .keys()
            .filter(|name| name.starts_with("src/__chigiri/types/"))
            .count(),
        19 + 65 + 1 + 1
    );
    assert!(files.contains_key("src/__chigiri/types/node.rs"));
    assert!(files.contains_key("src/__chigiri/types/search_result.rs"));
}
