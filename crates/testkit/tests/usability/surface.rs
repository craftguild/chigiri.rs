//! Pins the facade source surface, quick start, and immutable endpoint contract.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn facade_source() -> String {
    [
        "crates/facade/src/lib.rs",
        "crates/facade/src/endpoint.rs",
        "crates/facade/src/builder.rs",
        "crates/facade/src/components.rs",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(root().join(path)).unwrap())
    .collect()
}

fn public_builder_count(source: &str) -> usize {
    let tokens = source.split_whitespace().collect::<Vec<_>>();
    let mut count = 0;
    for (index, token) in tokens.iter().enumerate() {
        if *token != "pub" {
            continue;
        }
        let mut signature = &tokens[index + 1..];
        while matches!(
            signature.first().copied(),
            Some("const" | "async" | "unsafe")
        ) {
            signature = &signature[1..];
        }
        if signature.first().copied() == Some("extern") {
            signature = &signature[1..];
            if signature
                .first()
                .is_some_and(|token| token.starts_with('"'))
            {
                signature = &signature[1..];
            }
        }
        if signature.first().copied() == Some("fn")
            && signature
                .get(1)
                .is_some_and(|name| name.starts_with("builder<") || name.starts_with("builder("))
        {
            count += 1;
        }
    }
    count
}

#[test]
fn api_04_second_endpoint_build_does_not_expose_an_additional_entry_point() {
    let source = facade_source();
    let http = fs::read_to_string(root().join("crates/http/src/lib.rs")).unwrap();
    assert_eq!(public_builder_count(&source), 1);
    assert!(!source.contains("Endpoint::advanced"));
    assert!(!source.contains("pub fn advanced"));
    assert!(!source.contains("production_defaults"));
    assert!(!source.contains("ProductionEndpoint"));
    assert!(!http.contains("GraphQlEndpoint"));
    assert!(!http.contains("MissingContextEndpointBuilder"));
    assert!(!http.contains("EndpointBuilder"));
}

#[test]
fn api_09_quick_start_builds_a_default_endpoint() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let framework = readme
        .split_once("### Rust framework")
        .expect("README must describe the Rust framework workflow")
        .1
        .split("\n## ")
        .next()
        .unwrap();
    assert!(framework.contains("Endpoint::builder"));
    assert!(framework.contains("ObjectResolver<C, T, E>"));
    assert!(framework.contains("Resolvers::builder(map_app_error)"));
    assert!(framework.contains("context_factory(prepare_context)"));
    assert!(!framework.contains("chigiri_runtime"));
    assert!(!framework.contains("GraphQlEndpoint::builder"));
    assert!(!framework.contains("ProductionPreset"));
}

#[test]
fn inj_07_endpoint_is_immutable_after_build() {
    let endpoint = fs::read_to_string(root().join("crates/facade/src/endpoint.rs")).unwrap();
    let methods = endpoint
        .lines()
        .filter(|line| line.trim_start().starts_with("pub fn "))
        .collect::<String>();
    assert!(!methods.contains("inject"));
    assert!(!methods.contains("body_limit"));
    assert!(!methods.contains("rate_limit"));
    assert!(!methods.contains("components"));
}

#[test]
fn inj_08_does_not_expose_a_dynamic_registry() {
    let source = facade_source();
    assert!(!source.contains("std::any::Any"));
    assert!(!source.contains("downcast"));
    assert!(!source.contains("HashMap<String"));
    assert!(!source.contains("service_locator"));
}

#[test]
fn inj_11_rustdoc_and_ide_hide_advanced_input() {
    let source = facade_source();
    let crate_root = fs::read_to_string(root().join("crates/facade/src/lib.rs")).unwrap();
    assert!(crate_root.contains("#![deny(missing_docs)]"));
    assert!(!source.contains("advanced"));
    assert!(!source.contains("development_defaults"));
    assert!(!source.contains("production_defaults"));
}
