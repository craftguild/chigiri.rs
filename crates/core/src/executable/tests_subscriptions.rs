use super::{
    EXECUTABLE_DIRECTIVE_ARGUMENT_REQUIRED, EXECUTABLE_DIRECTIVE_LOCATION_INVALID,
    EXECUTABLE_FIELD_ARGUMENT_REQUIRED, EXECUTABLE_FIELD_ARGUMENT_UNKNOWN,
    EXECUTABLE_FIELD_NOT_DEFINED, EXECUTABLE_FIELD_SELECTION_REQUIRED,
    EXECUTABLE_ROOT_TYPE_MISSING, EXECUTABLE_SUBSCRIPTION_INTROSPECTION_ROOT,
    EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE, EXECUTABLE_SUBSCRIPTION_SINGLE_ROOT_FIELD,
    validate_executable_document,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{DiagnosticCode, ExecutionLimits, SourceId};

fn schema(source: &str) -> crate::schema::SemanticSchema {
    let parsed =
        parse_type_system_document(SourceId::new(70), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let output = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.schema.unwrap()
}

fn subscription_schema() -> crate::schema::SemanticSchema {
    schema(
        r#"
        schema { query: Query subscription: Subscription }
        directive @queryOnly(flag: Boolean!) on QUERY
        interface SubscriptionContract { event(id: ID!): Event }
        type Query { ok: Boolean }
        type Subscription implements SubscriptionContract {
          event(id: ID!): Event
          other: Event
        }
        type Event { id: ID! name: String }
        "#,
    )
}

fn validate(
    schema: &crate::schema::SemanticSchema,
    source: &str,
) -> super::ExecutableValidationOutput {
    let parsed =
        parse_executable_document(SourceId::new(71), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    validate_executable_document(
        schema,
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    )
}

fn codes(output: &super::ExecutableValidationOutput) -> Vec<DiagnosticCode> {
    output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn validates_and_selects_a_direct_subscription() {
    let output = validate(
        &subscription_schema(),
        r#"subscription Events { event(id: "room") { id name } }"#,
    );
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());

    let document = output.document.unwrap();
    let selected = document.select_operation(None).unwrap();
    assert_eq!(
        selected.operation.operation_type,
        crate::language::ast::OperationType::Subscription
    );
}

#[test]
fn aliases_and_merged_fields_form_one_response_name_group() {
    let output = validate(
        &subscription_schema(),
        r#"subscription {
          selected: event(id: "room") { id }
          selected: event(id: "room") { name }
        }"#,
    );

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn collects_named_and_inline_fragments_using_root_applicability() {
    for source in [
        r#"subscription { ...RootEvent }
           fragment RootEvent on SubscriptionContract { event(id: "room") { id } }"#,
        r#"subscription {
             ... on SubscriptionContract { event(id: "room") { id } }
           }"#,
    ] {
        let output = validate(&subscription_schema(), source);
        assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    }
}

#[test]
fn rejects_multiple_response_name_groups() {
    let output = validate(
        &subscription_schema(),
        r#"subscription {
          first: event(id: "room") { id }
          second: event(id: "room") { id }
        }"#,
    );

    assert_eq!(
        codes(&output),
        vec![EXECUTABLE_SUBSCRIPTION_SINGLE_ROOT_FIELD]
    );
    assert_eq!(
        output.diagnostics.iter().next().unwrap().message,
        "Subscription operation must have exactly one root field"
    );
}

#[test]
fn rejects_conditional_directives_across_root_traversal_forms() {
    let output = validate(
        &subscription_schema(),
        r#"subscription RootRules($enabled: Boolean!) {
          event(id: "room") @skip(if: $enabled) { id }
          ...RootEvent @include(if: $enabled)
          ... @skip(if: $enabled) { event(id: "room") { name } }
        }
        fragment RootEvent on Subscription {
          event(id: "room") @include(if: $enabled) { id }
        }"#,
    );
    let found = codes(&output);

    assert_eq!(
        found
            .iter()
            .filter(|code| **code == EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE)
            .count(),
        4
    );
    assert!(output.diagnostics.iter().all(|diagnostic| {
        diagnostic.code != EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE
            || diagnostic
                .message
                .ends_with("is not allowed on a subscription root selection")
    }));
}

#[test]
fn rejects_an_aliased_introspection_root_field() {
    let output = validate(&subscription_schema(), "subscription { root: __typename }");

    assert_eq!(
        codes(&output),
        vec![EXECUTABLE_SUBSCRIPTION_INTROSPECTION_ROOT]
    );
    assert_eq!(
        output.diagnostics.iter().next().unwrap().message,
        "`__typename` cannot be the root field of a subscription operation"
    );
}

#[test]
fn reports_a_missing_subscription_root() {
    let schema = schema("type Query { ok: Boolean }");
    let output = validate(&schema, "subscription { ok }");

    assert_eq!(codes(&output), vec![EXECUTABLE_ROOT_TYPE_MISSING]);
}

#[test]
fn preserves_ordinary_field_argument_and_directive_validation() {
    let schema = subscription_schema();
    let invalid_field = validate(&schema, "subscription { missing }");
    assert_eq!(codes(&invalid_field), vec![EXECUTABLE_FIELD_NOT_DEFINED]);

    let invalid_shape = validate(&schema, "subscription { event(extra: 1) @queryOnly }");
    let found = codes(&invalid_shape);
    for expected in [
        EXECUTABLE_DIRECTIVE_LOCATION_INVALID,
        EXECUTABLE_DIRECTIVE_ARGUMENT_REQUIRED,
        EXECUTABLE_FIELD_ARGUMENT_UNKNOWN,
        EXECUTABLE_FIELD_ARGUMENT_REQUIRED,
        EXECUTABLE_FIELD_SELECTION_REQUIRED,
    ] {
        assert!(found.contains(&expected), "missing {expected:?}: {found:?}");
    }
}
