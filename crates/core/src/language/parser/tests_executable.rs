use super::parse_executable_document;
use crate::language::ast::{Definition, ExecutableDefinition, OperationType, Selection};
use crate::language::ast::{StringStyle, TypeRefKind, Value};
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str) -> crate::language::ParseOutput {
    parse_executable_document(SourceId::new(7), source, &ExecutionLimits::unbounded())
}

#[test]
fn shorthand_and_three_operations() {
    let output =
        parse("{ viewer { id } } mutation Rename { rename } subscription Events { changed }");
    let document = output.document.expect("valid document");
    let operations: Vec<_> = document
        .executable_definitions()
        .map(|definition| match definition {
            ExecutableDefinition::Operation(operation) => operation,
            _ => panic!("expected an operation."),
        })
        .collect();

    assert!(operations[0].is_shorthand());
    assert_eq!(operations[0].operation_type, OperationType::Query);
    assert_eq!(operations[1].operation_type, OperationType::Mutation);
    assert_eq!(operations[2].operation_type, OperationType::Subscription);
    assert_eq!(operations[1].name.as_ref().unwrap().value, "Rename");
}

#[test]
fn parses_variable_types_defaults_and_directives() {
    let source = r#"
        "operation docs"
        query Fetch(
          "variable docs"
          $id: [ID!]! = ["book-01"] @fromClient(enabled: true)
        ) @first @second(value: 2) { book(id: $id) { id } }
        "fragment docs"
        fragment BookFields on Book @tag(name: "public") { title }
    "#;
    let output = parse(source);
    let document = output.document.expect("valid document");
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let variable = &operation.variable_definitions[0];

    assert_eq!(
        operation.description.as_ref().unwrap().value.value,
        "operation docs"
    );
    assert_eq!(
        variable.description.as_ref().unwrap().value.value,
        "variable docs"
    );
    assert_eq!(operation.directives[0].name.value, "first");
    assert_eq!(operation.directives[1].name.value, "second");
    assert!(matches!(variable.ty.kind, TypeRefKind::NonNull(_)));
    let Definition::Executable(ExecutableDefinition::Fragment(fragment)) = &document.definitions[1]
    else {
        panic!("expected a fragment.");
    };
    assert_eq!(
        fragment.description.as_ref().unwrap().value.value,
        "fragment docs"
    );
    assert!(operation.span.start < operation.operation_type_span.unwrap().start);
    assert!(variable.span.start < variable.variable.span.start);
}

#[test]
fn fields_aliases_arguments_and_fragments_preserve_all_three_source_forms_in_order() {
    let output = parse(
        r#"query Q($show: Boolean) {
          first: user(id: 1, id: 2) @a @b {
            ...Named @skip(if: false)
            ... on Admin @include(if: $show) { level }
            ... @group { name }
          }
        }
        fragment Named on User { id }
        "#,
    );
    let document = output.document.expect("valid document");
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let Selection::Field(field) = &operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    assert_eq!(field.alias.as_ref().unwrap().value, "first");
    assert_eq!(field.name.value, "user");
    assert_eq!(field.arguments[0].name.value, field.arguments[1].name.value);
    assert_eq!(field.directives[0].name.value, "a");
    assert_eq!(field.directives[1].name.value, "b");

    let nested = field.selection_set.as_ref().unwrap();
    assert!(matches!(nested.selections[0], Selection::FragmentSpread(_)));
    let Selection::InlineFragment(typed) = &nested.selections[1] else {
        panic!("expected a typed inline fragment");
    };
    assert_eq!(typed.type_condition.as_ref().unwrap().value, "Admin");
    let Selection::InlineFragment(untyped) = &nested.selections[2] else {
        panic!("expected an untyped inline fragment");
    };
    assert!(untyped.type_condition.is_none());
}

#[test]
fn preserves_all_input_value_lexemes_and_string_styles() {
    let output = parse(
        r#"query Values($v: Int) {
          field(input: {
            int: -2147483649, float: 1.250e+3,
            quoted: "text", block: """
              block
            """, yes: true, no: false, none: null, enum: MOBILE,
            list: [1, $v], object: { key: 1, key: 2 }
          })
        }"#,
    );
    let document = output.document.expect("valid document");
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };
    let Selection::Field(field) = &operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    let Value::Object(fields) = &field.arguments[0].value.value else {
        panic!("expected object value.");
    };

    assert!(matches!(&fields[0].value.value, Value::Int(raw) if raw == "-2147483649"));
    assert!(matches!(&fields[1].value.value, Value::Float(raw) if raw == "1.250e+3"));
    assert!(
        matches!(&fields[2].value.value, Value::String(value) if value.style == StringStyle::Quoted)
    );
    assert!(
        matches!(&fields[3].value.value, Value::String(value) if value.style == StringStyle::Block)
    );
    assert!(matches!(fields[4].value.value, Value::Boolean(true)));
    assert!(matches!(fields[5].value.value, Value::Boolean(false)));
    assert!(matches!(fields[6].value.value, Value::Null));
    assert!(matches!(fields[7].value.value, Value::Enum(_)));
    assert!(matches!(fields[8].value.value, Value::List(_)));
    let Value::Object(duplicates) = &fields[9].value.value else {
        panic!("expected nested object.");
    };
    assert_eq!(duplicates[0].name.value, duplicates[1].name.value);
}
