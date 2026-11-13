//! Preserve conservative ownership checks while locating later ordinary methods.

use std::fs;

use super::{Fixture, span};
use crate::doctor::correlation::{qualified, scope};

fn identified(source: &str) -> bool {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    let start = source.rfind("fn id").unwrap();
    let end = source[start..].find(" { todo!").unwrap() + start;
    let mut primary = span("src/lib.rs", true, "return signature", "expected Id");
    primary["byte_start"] = start.into();
    primary["byte_end"] = end.into();
    qualified::method(
        &primary,
        fixture.schema.object("User").unwrap(),
        "id",
        &fixture.sources,
    )
}

#[test]
fn later_method_ignores_prior_bodies_strings_characters_and_comments() {
    let source = concat!(
        "impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields {\n",
        "async fn name<'a>(&'a self) -> chigiri::FieldResult<String> {\n",
        "let text = \"{ impl fake \\\" }\"; let brace = '}'; let escaped = '\\'';\n",
        "// A closing brace } inside a line comment.\n",
        "/* { /* nested } */ } */ Ok(text.to_owned()) }\n",
        "async fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() }\n}",
    );
    assert!(identified(source));
}

#[test]
fn signatures_outside_the_selected_impl_and_ambiguous_source_decline() {
    for source in [
        "impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields {}\nasync fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() }",
        "impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields { fn outer() { async fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() } } }",
        "impl ordinary::UserResolver<(), Types, chigiri::FieldError> for Fields { async fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() } }",
        "impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields { async fn id<'a>(&'a self) -> /* unclear */ chigiri::FieldResult<String> { todo!() } }",
        "const RAW: &str = r#\"{\"#; impl ::generated::UserResolver<(), Types, chigiri::FieldError> for Fields { async fn id<'a>(&'a self) -> chigiri::FieldResult<String> { todo!() } }",
    ] {
        assert!(!identified(source), "{source}");
    }
}

#[test]
fn lexical_scope_has_depth_and_incomplete_literal_fallbacks() {
    for prefix in ["\"unterminated", "/* unterminated", "// hidden ", "'\\"] {
        let source = format!("{{ {prefix} fn id");
        assert_eq!(
            scope::enclosing_brace(&source, source.len() - 5),
            None,
            "{source}"
        );
    }
    let source = format!("{}fn id", "{".repeat(65));
    assert_eq!(scope::enclosing_brace(&source, 65), None);
}
