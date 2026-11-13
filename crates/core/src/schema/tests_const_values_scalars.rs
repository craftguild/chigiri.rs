use super::{
    SCHEMA_CONST_NULL_FOR_NON_NULL, SCHEMA_CONST_SCALAR_VALUE, SCHEMA_CONST_VALUE_VARIABLE,
    SCHEMA_INVALID_AST_SYNTAX, compose_schema,
};
use crate::language::ast::{Definition, Name, TypeSystemDefinition, Value, ValueNode};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId, Span};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

fn count(output: &super::SchemaBuildOutput, code: crate::DiagnosticCode) -> usize {
    output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .count()
}

#[test]
fn accepts_builtin_values_singleton_list_coercion_and_custom_scalar_shapes() {
    let document = parse(
        150,
        r#"
        scalar JSON
        directive @valid(
          min: Int = -2147483648
          max: Int = 2147483647
          promoted: Float = 2147483648
          exact: Float = 1606938044258990275541962092341162602522202993782792835301376
          decimal: Float = 1.25e2
          text: String = "text"
          flag: Boolean = true
          idText: ID = "4"
          idInt: ID = 99999999999999999999999999999999999
          list: [Int] = 1
          custom: JSON = { nested: [1, "x", null] }
        ) on OBJECT
        type Query @valid { ok: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn rejects_out_of_range_builtin_values_and_null_for_non_null_types() {
    let document = parse(
        151,
        r#"
        directive @bad(
          intRange: Int = 2147483648
          intKind: Int = 1.0
          floatKind: Float = "1"
          floatFinite: Float = 1e9999
          floatUnderflow: Float = 1e-400
          floatPrecision: Float = 1606938044258990275541962092341162602522202993782792835301377
          stringKind: String = true
          booleanKind: Boolean = "true"
          idKind: ID = 1.0
          required: Int! = null
          items: [Int] = [1, "x"]
        ) on OBJECT
        type Query { ok: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_CONST_SCALAR_VALUE), 10);
    assert_eq!(count(&output, SCHEMA_CONST_NULL_FOR_NON_NULL), 1);
}

#[test]
fn validates_ast_id_lexemes_before_composition() {
    let mut document = parse(
        152,
        "directive @id(value: ID = 1) on OBJECT type Query { ok: String }",
    );
    let definition = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Directive(definition)) => Some(definition),
            _ => None,
        })
        .unwrap();
    definition.arguments[0]
        .default_value
        .as_mut()
        .unwrap()
        .value = Value::Int("01".to_owned());

    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_INVALID_AST_SYNTAX), 1);
}

#[test]
fn rejects_variables_nested_inside_hand_built_custom_scalar_ast_values() {
    let mut document = parse(
        153,
        concat!(
            "scalar JSON ",
            "directive @json(value: JSON = { ok: 1 }) on OBJECT ",
            "type Query { ok: String }",
        ),
    );
    let variable_span = Span::new(SourceId::new(153), 32, 34);
    let variable = ValueNode::new(
        Value::Variable(Name::new(
            "x".to_owned(),
            Span::new(SourceId::new(153), 33, 34),
        )),
        variable_span,
    );
    let definition = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Directive(definition)) => Some(definition),
            _ => None,
        })
        .unwrap();
    let Value::Object(fields) = &mut definition.arguments[0]
        .default_value
        .as_mut()
        .unwrap()
        .value
    else {
        panic!("The default must be an object.");
    };
    fields[0].value = variable;

    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_CONST_VALUE_VARIABLE)
        .unwrap();

    assert!(output.schema.is_none());
    assert_eq!(diagnostic.span, Some(variable_span));
}
