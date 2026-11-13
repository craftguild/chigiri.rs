use crate::executable::{CoercedInput, CustomScalarRegistry, coerce_raw_input};
use crate::language::parse_type_system_document;
use crate::schema::compose_schema;
use crate::{ExecutionLimits, Object, SourceId, Value};

use super::plan::{metrics, reset_metrics};

#[test]
fn builds_wide_optional_input_plans_once_and_executes_empty_values() {
    const FIELDS: usize = 10_000;
    const OCCURRENCES: usize = 256;
    let mut source = String::from("input Wide {\n");
    for index in 0..FIELDS {
        source.push_str(&format!("field{index}: String\n"));
    }
    source.push_str("}\ntype Query { run(inputs: [Wide]): Boolean }");
    let parsed =
        parse_type_system_document(SourceId::new(42), &source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let built = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(built.is_valid(), "{:?}", built.diagnostics.into_vec());
    let schema = built.schema.unwrap();
    let expected = &schema
        .type_by_name("Query")
        .unwrap()
        .field_argument("run", "inputs")
        .unwrap()
        .ty;
    let raw = Value::List(
        (0..OCCURRENCES)
            .map(|_| Value::Object(Object::new()))
            .collect(),
    );

    reset_metrics();
    let output = coerce_raw_input(
        &schema,
        expected,
        raw,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
    );
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    let Some(CoercedInput::Value(Value::List(values))) = output.value else {
        panic!("expected a coerced wide list");
    };
    assert_eq!(values.len(), OCCURRENCES);
    assert!(
        values
            .iter()
            .all(|value| { matches!(value, Value::Object(object) if object.capacity() == 0) })
    );
    assert_eq!(
        metrics(),
        super::plan::PlanMetrics {
            plan_builds: 1,
            plan_fields: FIELDS,
            occurrence_fields: 0,
        }
    );
}
