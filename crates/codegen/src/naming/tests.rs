use super::{
    IdentifierCase, IdentifierScope, NAME_COLLISION, TYPE_REF_NESTING_LIMIT, canonical_type_ref,
    rust_identifier, snake_case, upper_camel_case,
};
use chigiri_core::language::ast::{Name, TypeRef};
use chigiri_core::{SourceId, Span};

fn span(start: usize, end: usize) -> Span {
    Span::new(SourceId::new(1), start, end)
}

fn named(name: &str) -> TypeRef {
    TypeRef::named(
        Name::new(name.to_owned(), span(0, name.len())),
        span(0, name.len()),
    )
}

#[test]
fn converts_leading_underscores_and_acronyms_to_rust_case() {
    assert_eq!(upper_camel_case("_URL_value"), "_UrlValue");
    assert_eq!(snake_case("__HTTP2ServerID"), "__http2_server_id");
    assert_eq!(upper_camel_case("userID"), "UserId");
}

#[test]
fn rust_keywords_convert_to_raw_identifiers() {
    assert_eq!(rust_identifier("type", IdentifierCase::Snake), "r#type");
    assert_eq!(rust_identifier("match", IdentifierCase::Snake), "r#match");
    assert_eq!(rust_identifier("type", IdentifierCase::UpperCamel), "Type");
}

#[test]
fn reserved_words_use_raw_identifiers() {
    let mut scope = IdentifierScope::default();
    let name = Name::new("self".to_owned(), span(0, 4));

    assert_eq!(
        scope.reserve(&name, IdentifierCase::Snake).unwrap(),
        "__chigiri_name_self"
    );

    let underscore = Name::new("_".to_owned(), span(5, 6));
    assert_eq!(
        scope
            .reserve(&underscore, IdentifierCase::UpperCamel)
            .unwrap(),
        "__ChigiriNameUnderscore"
    );
    assert_eq!(
        rust_identifier("Self", IdentifierCase::UpperCamel),
        "__ChigiriNameSelf"
    );
    assert_eq!(
        scope.reserve(&name, IdentifierCase::UpperCamel).unwrap(),
        "__ChigiriNameSelf"
    );
    let collision = Name::new("__ChigiriNameSelf".to_owned(), span(7, 23));
    assert!(
        scope
            .reserve(&collision, IdentifierCase::UpperCamel)
            .is_err()
    );
}

#[test]
fn detects_post_transformation_collisions_with_sf4006() {
    let mut scope = IdentifierScope::default();
    let first = Name::new("foo_bar".to_owned(), span(0, 7));
    let second = Name::new("fooBar".to_owned(), span(8, 14));

    assert_eq!(
        scope.reserve(&first, IdentifierCase::Snake).unwrap(),
        "foo_bar"
    );
    let error = scope.reserve(&second, IdentifierCase::Snake).unwrap_err();

    assert_eq!(error.code, NAME_COLLISION);
    assert_eq!(error.span, Some(second.span));
    assert!(error.message.contains("foo_bar"));
}

#[test]
fn nested_list_and_non_null_canonical_strings() {
    let item = TypeRef::non_null(named("User"), span(2, 7));
    let inner_list = TypeRef::list(item, span(1, 8));
    let required_list = TypeRef::non_null(inner_list, span(1, 9));
    let outer_list = TypeRef::list(required_list, span(0, 10));
    let type_ref = TypeRef::non_null(outer_list, span(0, 11));

    assert_eq!(canonical_type_ref(&type_ref, 5).unwrap(), "[[User!]!]!");
}

#[test]
fn type_reference_nesting_reports_the_limit_deterministically() {
    let item = TypeRef::non_null(named("String"), span(1, 8));
    let type_ref = TypeRef::list(item, span(0, 9));

    let error = canonical_type_ref(&type_ref, 1).unwrap_err();

    assert_eq!(error.code, TYPE_REF_NESTING_LIMIT);
    assert_eq!(error.span, Some(span(0, 6)));
}
