use super::parse_executable_document;
use crate::language::ast::{Definition, ExecutableDefinition, Selection};
use crate::{ExecutionLimits, SourceId};

#[test]
fn accepts_keyword_names_and_explicit_anonymous_operations() {
    let output = parse_executable_document(
        SourceId::new(13),
        "query($on: Int) @query(on: $on) { mutation: fragment(query: $on) plain }",
        &ExecutionLimits::unbounded(),
    );
    let document = output.document.expect("valid executable document");
    let Definition::Executable(ExecutableDefinition::Operation(operation)) =
        &document.definitions[0]
    else {
        panic!("expected an operation.");
    };

    assert!(operation.name.is_none());
    assert_eq!(operation.variable_definitions[0].variable.value, "on");
    assert_eq!(operation.directives[0].name.value, "query");
    let Selection::Field(aliased) = &operation.selection_set.selections[0] else {
        panic!("expected a field.");
    };
    assert_eq!(aliased.alias.as_ref().unwrap().value, "mutation");
    assert_eq!(aliased.name.value, "fragment");
    assert!(aliased.selection_set.is_none());
    assert!(matches!(
        operation.selection_set.selections[1],
        Selection::Field(_)
    ));
}
