//! Uses real rustc diagnostics to correlate missing groups, trait fields, and output types with SDL.

use std::fs;

use super::doctor_fixture::{CONSUMER, prepare};
use super::fixture::{Fixture, report};

const ID: &str = "async fn id<'a>(&'a self, _: ::chigiri::FieldContext<'a, ()>, user: &'a User) -> ::chigiri::FieldResult<::chigiri::Id> { Ok(user.id.clone()) }";
const NAME: &str = "async fn name<'a>(&'a self, _: ::chigiri::FieldContext<'a, ()>, user: &'a User) -> ::chigiri::FieldResult<String> { Ok(user.name.clone()) }";

fn traits(methods: &str) -> Fixture {
    let fixture = prepare("");
    fs::write(
        fixture.root.join("graphql/schema.graphql"),
        "type Query { value: String }\ntype User { id: ID!, name: String! }\n",
    )
    .unwrap();
    fixture.success("update");
    let mut source = CONSUMER[..CONSUMER.find("pub fn resolvers").unwrap()].replace(
        "SchemaTypes for Types {}",
        "SchemaTypes for Types { type User = User; }",
    );
    source.push_str(&format!(
        "\npub struct User {{ pub id: ::chigiri::Id, pub name: String }}\n\
         pub struct UserFields;\n\
         impl ::contract::UserResolver<(), Types, ::chigiri::FieldError> for UserFields {{\n{methods}\n}}\n"
    ));
    fs::write(fixture.root.join("consumer/src/lib.rs"), source).unwrap();
    fixture
}

fn mapped(fixture: &Fixture, native: &str, hint: &str, coordinate: Option<&str>) {
    let output = fixture.run("doctor");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = report(&output);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|record| record["code"] == native && record["origin"] == "rustc"),
        "{value}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|record| record["code"] == hint)
            .count(),
        1,
        "{value}"
    );
    let supplement = diagnostics
        .iter()
        .find(|record| record["code"] == hint)
        .unwrap_or_else(|| panic!("{value}"));
    assert_eq!(supplement["labels"][0]["path"], "consumer/src/lib.rs");
    if let Some(coordinate) = coordinate {
        assert_eq!(supplement["coordinate"], coordinate, "{value}");
        assert!(
            diagnostics
                .iter()
                .any(|record| record["code"] == native && record["coordinate"] == coordinate),
            "{value}"
        );
    }
}

#[test]
fn diag_08_real_consumer_reports_the_next_missing_resolver_group_method() {
    let fixture = prepare("");
    fs::write(
        fixture.root.join("consumer/src/lib.rs"),
        CONSUMER.replace(".with_query(Query)", ""),
    )
    .unwrap();
    mapped(&fixture, "E0599", "SF4018", None);
}

#[test]
fn diag_08_maps_missing_real_consumer_trait_fields() {
    let fixture = traits(ID);
    mapped(&fixture, "E0046", "SF4018", Some("User.name"));
}

#[test]
fn diag_09_maps_real_consumer_trait_output_mismatches() {
    let wrong_id = ID
        .replace("FieldResult<::chigiri::Id>", "FieldResult<String>")
        .replace("Ok(user.id.clone())", "Ok(user.id.to_string())");
    let fixture = traits(&format!("{wrong_id}\n{NAME}"));
    mapped(&fixture, "E0271", "SF4019", Some("User.id"));
}

#[test]
fn diag_08_maps_follow_up_groups_to_the_preceding_missing_method() {
    let fixture = traits(&format!("{ID}\n{NAME}"));
    let path = fixture.root.join("consumer/src/lib.rs");
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str("pub fn assembly() -> ::contract::Resolvers<(), Types> { ::contract::Resolvers::builder(std::convert::identity::<chigiri::FieldError>).with_user(UserFields).build() }\n");
    fs::write(path, source).unwrap();
    mapped(&fixture, "E0599", "SF4018", None);
}
