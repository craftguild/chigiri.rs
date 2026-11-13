//! Exercise successful/error result positions and method order through real CLI diagnostics.

use std::fs;

use serde_json::Value;

use super::doctor_fixture::{inputs, prepare};
use super::fixture::{Fixture, report};

fn fixture() -> Fixture {
    let fixture = prepare("");
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { first: String! second: Int! third: Int! }\n",
    )
    .unwrap();
    fixture.success("update");
    fixture
}

fn source(selected: &str, output: &str, error: &str, order: &[&str]) -> String {
    let mut source = String::from(
        "pub struct Types; impl ::contract::SchemaTypes for Types {}\n\
         pub struct Fields;\n\
         impl ::contract::QueryResolver<(), Types, ::chigiri::FieldError> for Fields {\n",
    );
    for name in order {
        let (result, body) = if *name == selected {
            (
                output,
                if output == "String" {
                    "Ok(String::new())"
                } else {
                    "Ok(2)"
                },
            )
        } else if *name == "first" {
            (
                "String",
                "let value = if true { \"{first}\" } else { \"}\" }; Ok(value.to_owned())",
            )
        } else {
            ("i32", "let value = { 2 }; Ok(value)")
        };
        let error = if *name == selected {
            error
        } else {
            "::chigiri::FieldError"
        };
        source.push_str(&format!(
            "async fn {name}<'a>(&'a self, _: ::chigiri::FieldContext<'a, ()>) -> Result<{result}, {error}> {{ {body} }}\n",
        ));
    }
    source.push_str("}\n");
    source
}

fn check(fixture: &Fixture, lane: &str, source: &str) -> Value {
    fs::write(fixture.root.join("consumer/src/lib.rs"), source).unwrap();
    let original = inputs(fixture);
    let sdk = fixture.snapshot();
    let output = fixture.execute(fixture.command("doctor").env("RUSTUP_TOOLCHAIN", lane));
    assert_eq!(output.status.code(), Some(1), "{lane}: {output:?}");
    let value = report(&output);
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| { record["code"] == "E0271" && record["origin"] == "rustc" }),
        "{lane}: {value}"
    );
    assert!(
        !value.to_string().contains(fixture.root.to_str().unwrap()),
        "{value}"
    );
    assert_eq!(inputs(fixture), original);
    assert_eq!(fixture.snapshot(), sdk);
    value
}

#[test]
fn r53_03_error_only_mismatch_keeps_rustc_without_success_output_advice() {
    let fixture = fixture();
    let lane = "1.95.0";
    for (field, output) in [("first", "String"), ("third", "i32")] {
        let value = check(
            &fixture,
            lane,
            &source(
                field,
                output,
                "std::io::Error",
                &["first", "second", "third"],
            ),
        );
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|record| { record["code"] != "SF4019" }),
            "{lane}: {value}"
        );
    }
}

#[test]
fn r53_03_04_output_guidance_survives_error_mismatch_and_method_reordering() {
    let fixture = fixture();
    let lane = "1.95.0";
    for error in ["::chigiri::FieldError", "std::io::Error"] {
        for order in [
            ["first", "second", "third"],
            ["third", "first", "second"],
            ["second", "third", "first"],
        ] {
            let value = check(&fixture, lane, &source("third", "String", error, &order));
            let records = value["diagnostics"].as_array().unwrap();
            let hints = records
                .iter()
                .filter(|record| record["code"] == "SF4019")
                .collect::<Vec<_>>();
            assert_eq!(hints.len(), 1, "{lane}, {error}, {order:?}: {value}");
            assert_eq!(hints[0]["coordinate"], "Query.third", "{value}");
            let help = hints[0]["help"].to_string();
            assert!(
                help.contains("Rust type `i32`") && help.contains("GraphQL type `Int!`"),
                "{value}"
            );
            assert!(
                records.iter().any(
                    |record| record["code"] == "E0271" && record["coordinate"] == "Query.third"
                ),
                "{value}"
            );
        }
    }
}
