use super::builtins::empty_schema_with_builtins;
use super::{SchemaOrigin, SemanticTypeRefKind, TypeKind};
use crate::language::ast::{DirectiveLocation, Value};

#[test]
fn injects_specified_builtins_in_stable_order_and_supports_lookup() {
    let schema = empty_schema_with_builtins();
    let type_names = schema
        .types()
        .iter()
        .map(|named| named.name())
        .collect::<Vec<_>>();
    let directive_names = schema
        .directive_definitions()
        .iter()
        .map(|directive| directive.name())
        .collect::<Vec<_>>();

    assert_eq!(type_names, ["Int", "Float", "String", "Boolean", "ID"]);
    assert_eq!(
        directive_names,
        ["skip", "include", "deprecated", "specifiedBy", "oneOf"]
    );
    assert_eq!(schema.type_by_name("ID").unwrap().kind(), TypeKind::Scalar);
    assert_eq!(
        schema.type_by_name("ID").unwrap().origins(),
        [SchemaOrigin::BuiltIn]
    );
    assert_eq!(
        schema.directive_definition("oneOf").unwrap().locations()[0].value,
        DirectiveLocation::InputObject
    );
}

#[test]
fn preserves_builtin_directive_signatures() {
    let skip = empty_schema_with_builtins()
        .directive_definition("skip")
        .unwrap()
        .clone();
    let argument = &skip.arguments()[0];
    let SemanticTypeRefKind::NonNull(inner) = &argument.ty.kind else {
        panic!("the `if` argument must be non-null");
    };
    assert!(matches!(
        inner.kind,
        SemanticTypeRefKind::Named(ref name) if name == "Boolean"
    ));

    let deprecated = empty_schema_with_builtins()
        .directive_definition("deprecated")
        .unwrap()
        .clone();
    assert!(matches!(
        deprecated.arguments()[0].default_value,
        Some(Value::String(ref value)) if value.value == "No longer supported"
    ));
}
