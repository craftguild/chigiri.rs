use super::value_rules::{VARIABLE_POSITION_INVALID, validate_values};
use super::{
    CustomScalarRegistry, INPUT_CUSTOM_SCALAR_VALUE, INPUT_DUPLICATE_FIELD, INPUT_ENUM_VALUE,
    INPUT_NULL_FOR_NON_NULL, INPUT_OBJECT_VALUE, INPUT_ONE_OF_VALUE, INPUT_REQUIRED_VALUE_MISSING,
    INPUT_SCALAR_VALUE, INPUT_UNKNOWN_FIELD, ScalarCoercionError,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::{SemanticSchema, compose_schema};
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits, SourceId, Value as CoreValue};

fn schema() -> SemanticSchema {
    let source = r#"
        scalar Token
        enum Color { RED GREEN }
        input Config { required: Int!, color: Color, ids: [ID!] }
        input Choice @oneOf { id: ID, name: String }
        directive @tag(flag: Boolean!, count: Int = 2) on QUERY | FIELD | FRAGMENT_DEFINITION
        type Query {
          run(config: Config, choice: Choice, count: Int!, defaulted: Int! = 1, text: String, ratio: Float, key: ID, token: Token): Boolean
        }
    "#;
    let parsed =
        parse_type_system_document(SourceId::new(51), source, &ExecutionLimits::unbounded());
    let output = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.schema.unwrap()
}

fn validate(source: &str, maximum: usize) -> (bool, DiagnosticBag) {
    validate_with_scalars(source, maximum, &CustomScalarRegistry::new())
}

fn validate_with_scalars(
    source: &str,
    maximum: usize,
    scalars: &CustomScalarRegistry,
) -> (bool, DiagnosticBag) {
    let parsed =
        parse_executable_document(SourceId::new(52), source, &ExecutionLimits::unbounded());
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics.into_vec());
    let mut diagnostics = DiagnosticBag::new(maximum);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = maximum;
    let valid = validate_values(
        &schema(),
        &parsed.document.unwrap(),
        scalars,
        &limits,
        &mut diagnostics,
    );
    (valid, diagnostics)
}

fn codes(diagnostics: &DiagnosticBag) -> Vec<DiagnosticCode> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn validates_builtin_enum_input_object_and_list_item_values() {
    let (_, diagnostics) = validate(
        r#"query Q {
          run(config: {
            required: "bad"
            unknown: 1
            color: BLUE
            ids: [null]
            required: 2
          })
        }"#,
        20,
    );
    let found = codes(&diagnostics);

    assert!(found.contains(&INPUT_DUPLICATE_FIELD));
    assert!(found.contains(&INPUT_UNKNOWN_FIELD));
    assert!(found.contains(&INPUT_SCALAR_VALUE));
    assert!(found.contains(&INPUT_ENUM_VALUE));
    assert!(found.contains(&INPUT_NULL_FOR_NON_NULL));

    let (valid, diagnostics) = validate(
        r#"query Q { run(config: { required: 1, ids: "42", color: RED }) }"#,
        20,
    );
    assert!(valid, "{:?}", diagnostics.into_vec());
}

#[test]
fn float_integer_literals_require_exact_f64_representation() {
    let (valid, diagnostics) = validate("query Q { run(count: 1, ratio: 9007199254740992) }", 10);
    assert!(valid, "{:?}", diagnostics.into_vec());

    let two_to_200 = "1606938044258990275541962092341162602522202993782792835301376";
    let (valid, diagnostics) = validate(
        &format!("query Q {{ run(count: 1, ratio: {two_to_200}) }}"),
        10,
    );
    assert!(valid, "{:?}", diagnostics.into_vec());

    let (valid, diagnostics) = validate("query Q { run(count: 1, ratio: 9007199254740993) }", 10);
    assert!(!valid);
    assert_eq!(codes(&diagnostics), vec![INPUT_SCALAR_VALUE]);

    let (valid, diagnostics) = validate("query Q { run(count: 1, ratio: 0.0) }", 10);
    assert!(valid, "{:?}", diagnostics.into_vec());
    let (valid, diagnostics) = validate("query Q { run(count: 1, ratio: 1e-400) }", 10);
    assert!(!valid);
    assert_eq!(codes(&diagnostics), vec![INPUT_SCALAR_VALUE]);

    let two_to_200_plus_one = "1606938044258990275541962092341162602522202993782792835301377";
    let (valid, diagnostics) = validate(
        &format!("query Q {{ run(count: 1, ratio: {two_to_200_plus_one}) }}"),
        10,
    );
    assert!(!valid);
    assert_eq!(codes(&diagnostics), vec![INPUT_SCALAR_VALUE]);
}

#[test]
fn id_literals_accept_integers_outside_the_i128_range() {
    for value in [
        "170141183460469231731687303715884105728",
        "-170141183460469231731687303715884105729",
    ] {
        let (valid, diagnostics) =
            validate(&format!("query Q {{ run(count: 1, key: {value}) }}"), 10);
        assert!(valid, "{value}: {:?}", diagnostics.into_vec());
    }
}

#[test]
fn distinguishes_input_object_shape_from_missing_required_fields() {
    let (_, diagnostics) = validate("query Q { run(config: 1) }", 10);
    assert_eq!(codes(&diagnostics), vec![INPUT_OBJECT_VALUE]);

    let (_, diagnostics) = validate("query Q { run(config: {}) }", 10);
    assert_eq!(codes(&diagnostics), vec![INPUT_REQUIRED_VALUE_MISSING]);
}

#[test]
fn one_of_literal_requires_exactly_one_non_null() {
    let (_, null) = validate("query Q { run(choice: { id: null }) }", 10);
    assert_eq!(codes(&null), vec![INPUT_ONE_OF_VALUE]);

    let (_, multiple) = validate(r#"query Q { run(choice: { id: "1", name: "A" }) }"#, 10);
    assert_eq!(codes(&multiple), vec![INPUT_ONE_OF_VALUE]);
}

#[test]
fn directive_arguments_use_the_same_literal_validation_rules() {
    let (_, diagnostics) = validate("query Q @tag(flag: 1) { run(count: 1) }", 10);
    assert_eq!(codes(&diagnostics), vec![INPUT_SCALAR_VALUE]);
}

#[test]
fn validates_non_null_defaults_after_matching_wrappers() {
    let (_, invalid) = validate("query Q($x: Int) { run(count: $x) }", 10);
    assert_eq!(codes(&invalid), vec![VARIABLE_POSITION_INVALID]);

    let (valid, diagnostics) = validate(
        "query Q($a: Int = 1, $b: Int) { run(count: $a, defaulted: $b) }",
        10,
    );
    assert!(valid, "{:?}", diagnostics.into_vec());

    let (_, wrappers) = validate(
        "query Q($ids: [Int]) { run(config: { required: 1, ids: $ids }) }",
        10,
    );
    assert_eq!(codes(&wrappers), vec![VARIABLE_POSITION_INVALID]);
}

#[test]
fn fragment_variable_usage_is_validated_against_the_operation_definition() {
    let (_, diagnostics) = validate(
        "query Q($x: String!) { ...F } fragment F on Query { run(count: $x) }",
        10,
    );
    assert_eq!(codes(&diagnostics), vec![VARIABLE_POSITION_INVALID]);
}

#[test]
fn distinguishes_variable_uses_with_the_same_span_by_node_identity() {
    use crate::language::ast::{Definition, ExecutableDefinition, Selection, Value};

    let parsed = parse_executable_document(
        SourceId::new(53),
        "query Q($x: Int!) { run(text: $x) run(count: $x) }",
        &ExecutionLimits::unbounded(),
    );
    let mut document = parsed.document.unwrap();
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &mut document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let Selection::Field(first) = &operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    let Value::Variable(first_variable) = &first.arguments[0].value.value else {
        panic!("expected a variable.");
    };
    let shared_span = first_variable.span;
    let Selection::Field(second) = &mut operation.selection_set.selections[1] else {
        panic!("expected a field.");
    };
    let Value::Variable(second_variable) = &mut second.arguments[0].value.value else {
        panic!("expected a variable.");
    };
    second_variable.span = shared_span;

    let mut diagnostics = DiagnosticBag::new(10);
    let valid = validate_values(
        &schema(),
        &document,
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
        &mut diagnostics,
    );
    assert!(!valid);
    assert_eq!(codes(&diagnostics), vec![VARIABLE_POSITION_INVALID]);
}

#[test]
fn direct_one_of_variables_must_be_declared_non_null() {
    let (_, nullable) = validate(r#"query Q($id: ID = "1") { run(choice: { id: $id }) }"#, 10);
    assert_eq!(codes(&nullable), vec![VARIABLE_POSITION_INVALID]);

    let (valid, diagnostics) = validate("query Q($id: ID!) { run(choice: { id: $id }) }", 10);
    assert!(valid, "{:?}", diagnostics.into_vec());
}

#[test]
fn custom_scalar_variables_must_match_their_structural_type_positions() {
    let (_, diagnostics) = validate(
        "query Q($raw: Int) { run(count: 1, token: { raw: $raw }) }",
        10,
    );
    assert_eq!(codes(&diagnostics), vec![VARIABLE_POSITION_INVALID]);

    let (valid, diagnostics) = validate(
        "query Q($raw: Token) { run(count: 1, token: { raw: $raw }) }",
        10,
    );
    assert!(valid, "{:?}", diagnostics.into_vec());
}

#[test]
fn passes_custom_scalar_constant_literals_to_the_callback() {
    let mut scalars = CustomScalarRegistry::new();
    scalars.register("Token", |_, value| {
        if value.as_str() == Some("bad") {
            Err(ScalarCoercionError::public("bad token"))
        } else {
            Ok(value)
        }
    });

    let (_, rejected) =
        validate_with_scalars(r#"query Q { run(count: 1, token: "bad") }"#, 10, &scalars);
    assert_eq!(codes(&rejected), vec![INPUT_CUSTOM_SCALAR_VALUE]);
    assert_eq!(rejected.iter().next().unwrap().message, "bad token");

    let (valid, accepted) =
        validate_with_scalars(r#"query Q { run(count: 1, token: "good") }"#, 10, &scalars);
    assert!(valid, "{:?}", accepted.into_vec());

    let (_, strict) = validate(r#"query Q { run(count: 1, token: "good") }"#, 10);
    assert_eq!(codes(&strict), vec![INPUT_CUSTOM_SCALAR_VALUE]);
}

#[test]
fn rejects_null_custom_scalar_results_and_duplicate_object_fields() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut null_scalars = CustomScalarRegistry::new();
    null_scalars.register("Token", |_, _| Ok(CoreValue::Null));
    let (_, null) = validate_with_scalars(
        r#"query Q { run(count: 1, token: "value") }"#,
        10,
        &null_scalars,
    );
    assert_eq!(codes(&null), vec![INPUT_CUSTOM_SCALAR_VALUE]);

    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    let mut object_scalars = CustomScalarRegistry::new();
    object_scalars.register("Token", move |_, value| {
        callback_calls.fetch_add(1, Ordering::Relaxed);
        Ok(value)
    });
    let (_, duplicate) = validate_with_scalars(
        "query Q { run(count: 1, token: { raw: 1, raw: 2 }) }",
        10,
        &object_scalars,
    );
    assert_eq!(codes(&duplicate), vec![INPUT_DUPLICATE_FIELD]);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn wide_optional_input_objects_validate_without_visiting_absent_fields() {
    use std::fmt::Write as _;

    let width = 5_000;
    let mut schema_source = String::from("input Wide {");
    for index in 0..width {
        write!(&mut schema_source, " f{index}: Int").unwrap();
    }
    schema_source.push_str(" } type Query { run(input: Wide): Boolean }");
    let parsed_schema = parse_type_system_document(
        SourceId::new(54),
        &schema_source,
        &ExecutionLimits::unbounded(),
    );
    let schema = compose_schema(
        &[parsed_schema.document.unwrap()],
        &ExecutionLimits::unbounded(),
    )
    .schema
    .unwrap();
    let query = format!("query Q {{ {} }}", "run(input: {}) ".repeat(width));
    let parsed =
        parse_executable_document(SourceId::new(55), &query, &ExecutionLimits::unbounded());
    let mut diagnostics = DiagnosticBag::new(10);
    let valid = validate_values(
        &schema,
        &parsed.document.unwrap(),
        &CustomScalarRegistry::new(),
        &ExecutionLimits::unbounded(),
        &mut diagnostics,
    );

    assert!(valid, "{:?}", diagnostics.into_vec());
}

#[test]
fn indexes_introspection_arguments() {
    let (_, diagnostics) = validate(
        "query Q($name: String) { __type(name: $name) { fields(includeDeprecated: 1) { name } } }",
        10,
    );
    let found = codes(&diagnostics);
    assert!(found.contains(&VARIABLE_POSITION_INVALID));
    assert!(found.contains(&INPUT_SCALAR_VALUE));

    let (valid, diagnostics) = validate(
        "query Q($name: String!) { __type(name: $name) { fields(includeDeprecated: false) { name } } }",
        10,
    );
    assert!(valid, "{:?}", diagnostics.into_vec());
}

#[test]
fn reports_truncation_as_the_first_finding() {
    let (valid, diagnostics) = validate(r#"query Q { run(count: "bad") }"#, 0);

    assert!(!valid);
    assert!(diagnostics.is_empty());
    assert!(diagnostics.was_truncated());
}
