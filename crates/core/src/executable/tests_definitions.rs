use crate::executable::{
    EXECUTABLE_AST_NESTING_LIMIT, EXECUTABLE_DUPLICATE_FRAGMENT_NAME,
    EXECUTABLE_DUPLICATE_OPERATION_NAME, EXECUTABLE_INVALID_AST_SYNTAX,
    EXECUTABLE_LONE_ANONYMOUS_OPERATION, EXECUTABLE_TYPE_SYSTEM_DEFINITION,
    OPERATION_NAME_REQUIRED, OPERATION_NOT_FOUND, validate_executable_document,
};
use crate::language::ast::{
    Definition, Description, ExecutableDefinition, Selection, StringStyle, StringValue,
};
use crate::language::{parse_document, parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{ExecutionLimits, SourceId};

fn schema() -> crate::schema::SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(1),
        "type Query { ok: Boolean } type Mutation { ok: Boolean } type Subscription { ok: Boolean }",
        &ExecutionLimits::unbounded(),
    );
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

fn validate(source: &str) -> super::ExecutableValidationOutput {
    let parsed = parse_executable_document(SourceId::new(2), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    )
}

#[test]
fn rejects_duplicate_operation_and_fragment_names_and_non_lone_anonymous_operations() {
    let output = validate(
        "query A { ok } query A { ok } { ok } fragment F on Query { ok } fragment F on Query { ok }",
    );
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(!output.is_valid());
    assert!(codes.contains(&EXECUTABLE_DUPLICATE_OPERATION_NAME));
    assert!(codes.contains(&EXECUTABLE_DUPLICATE_FRAGMENT_NAME));
    assert!(codes.contains(&EXECUTABLE_LONE_ANONYMOUS_OPERATION));
}

#[test]
fn selects_operations_by_reusing_the_validated_document() {
    let output = validate("query A { ok } mutation B { ok }");
    let document = output.document.unwrap();

    assert_eq!(document.operations().count(), 2);
    assert_eq!(
        document.select_operation(None).unwrap_err().code,
        OPERATION_NAME_REQUIRED
    );
    assert_eq!(
        document.select_operation(Some("Missing")).unwrap_err().code,
        OPERATION_NOT_FOUND
    );
    assert_eq!(
        document
            .select_operation(Some("B"))
            .unwrap()
            .operation
            .name
            .as_ref()
            .unwrap()
            .value,
        "B"
    );
}

#[test]
fn operation_selection_accepts_validated_subscriptions() {
    let output = validate("subscription Events { ok }");
    assert!(output.is_valid());
    assert_eq!(
        output
            .document
            .unwrap()
            .select_operation(None)
            .unwrap()
            .operation
            .operation_type,
        crate::language::ast::OperationType::Subscription
    );
}

#[test]
fn validates_operation_names_and_selection_nesting() {
    let parsed = parse_executable_document(
        SourceId::new(3),
        "query A { ok }",
        &ExecutionLimits::unbounded(),
    );
    let mut document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &mut document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    operation.name.as_mut().unwrap().value = "bad-name".to_owned();
    let output = validate_executable_document(&schema(), &document, &ExecutionLimits::unbounded());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_INVALID_AST_SYNTAX
    );
    assert!(output.document.is_none());

    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &mut document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    operation.name.as_mut().unwrap().value = "A".to_owned();
    let nested = operation.selection_set.clone();
    let Selection::Field(root) = &mut operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    root.selection_set = Some(nested);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_parser_nesting = 1;
    let output = validate_executable_document(&schema(), &document, &limits);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_AST_NESTING_LIMIT
    );
    assert!(output.document.is_none());
}

#[test]
fn zero_diagnostic_limit_still_rejects_an_invalid_document() {
    let parsed = parse_executable_document(
        SourceId::new(4),
        "query A { ok } query A { ok }",
        &ExecutionLimits::unbounded(),
    );
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 0;
    let output = validate_executable_document(&schema(), &parsed.document.unwrap(), &limits);

    assert!(output.document.is_none());
    assert!(output.diagnostics.is_empty());
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn rejects_manually_constructed_empty_documents() {
    let document = crate::language::ast::Document {
        definitions: Vec::new(),
        span: crate::Span::new(SourceId::new(5), 0, 0),
    };
    let output = validate_executable_document(&schema(), &document, &ExecutionLimits::unbounded());

    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_INVALID_AST_SYNTAX
    );
    assert!(output.document.is_none());
}

#[test]
fn rejects_shorthand_operations_with_manually_constructed_descriptions() {
    let parsed =
        parse_executable_document(SourceId::new(6), "{ ok }", &ExecutionLimits::unbounded());
    let mut document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &mut document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    operation.description = Some(Description::new(
        StringValue::new("invalid", StringStyle::Quoted),
        operation.span,
    ));

    let output = validate_executable_document(&schema(), &document, &ExecutionLimits::unbounded());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_INVALID_AST_SYNTAX
    );
}

#[test]
fn validates_hand_built_fragment_spread_ast() {
    let parsed = parse_executable_document(
        SourceId::new(7),
        "query Q { ...F } fragment F on Query { ok }",
        &ExecutionLimits::unbounded(),
    );
    let mut document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &mut document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let Selection::FragmentSpread(spread) = &mut operation.selection_set.selections[0] else {
        panic!("expected fragment spread.");
    };
    spread.name.value = "on".to_owned();

    let output = validate_executable_document(&schema(), &document, &ExecutionLimits::unbounded());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_INVALID_AST_SYNTAX
    );
}

#[test]
fn fragment_validation_stops_when_the_diagnostic_limit_is_reached() {
    let parsed = parse_executable_document(
        SourceId::new(8),
        "query A { ...Missing } query A { ...AlsoMissing }",
        &ExecutionLimits::unbounded(),
    );
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = validate_executable_document(&schema(), &parsed.document.unwrap(), &limits);

    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_DUPLICATE_OPERATION_NAME
    );
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn executable_input_rejects_type_system_definitions() {
    let parsed = parse_document(
        SourceId::new(9),
        "query Q { ok } type Extra { ok: Boolean }",
        &ExecutionLimits::unbounded(),
    );
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());

    let output = validate_executable_document(
        &schema(),
        &parsed.document.unwrap(),
        &ExecutionLimits::unbounded(),
    );
    assert!(!output.is_valid());
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        EXECUTABLE_TYPE_SYSTEM_DEFINITION
    );
}
