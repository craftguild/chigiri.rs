use std::fmt::Write as _;

use super::value_rules::metrics::{VisitMetrics, get, reset};
use super::value_rules::validate_values;
use super::{CustomScalarRegistry, INPUT_DUPLICATE_FIELD, INPUT_UNKNOWN_FIELD};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{DiagnosticBag, ExecutionLimits, SourceId};

fn schema() -> crate::schema::SemanticSchema {
    let parsed = parse_type_system_document(
        SourceId::new(56),
        "scalar Token input Config { known: Int } type Query { run(config: Config, token: Token): Boolean }",
        &ExecutionLimits::unbounded(),
    );
    compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded())
        .schema
        .unwrap()
}

fn validate(source: &str, scalars: &CustomScalarRegistry) -> DiagnosticBag {
    let parsed =
        parse_executable_document(SourceId::new(57), source, &ExecutionLimits::unbounded());
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let mut diagnostics = DiagnosticBag::new(1);
    let _ = validate_values(
        &schema(),
        &parsed.document.unwrap(),
        scalars,
        &limits,
        &mut diagnostics,
    );
    diagnostics
}

#[test]
fn input_object_validation_stops_after_reaching_the_diagnostic_limit() {
    let mut query = String::from("query Q { run(config: {");
    for index in 0..10_000 {
        write!(&mut query, " bad{index}: 1").unwrap();
    }
    query.push_str(" }) }");

    reset();
    let diagnostics = validate(&query, &CustomScalarRegistry::new());

    assert_eq!(diagnostics.iter().next().unwrap().code, INPUT_UNKNOWN_FIELD);
    assert_eq!(get().input_fields, 1);
}

#[test]
fn custom_scalar_stops_remaining_fields_at_the_second_diagnostic_limit() {
    let mut query = String::from("query Q { run(token: { raw: 1 raw: 2");
    for index in 0..10_000 {
        write!(&mut query, " tail{index}: 1").unwrap();
    }
    query.push_str(" }) }");
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("Token", |_, value| Ok(value));

    reset();
    let diagnostics = validate(&query, &scalars);

    assert_eq!(
        diagnostics.iter().next().unwrap().code,
        INPUT_DUPLICATE_FIELD
    );
    assert_eq!(
        get(),
        VisitMetrics {
            input_fields: 0,
            custom_fields: 2,
        }
    );
}
