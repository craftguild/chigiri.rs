//! Build introspection values for directive metadata and defaults.

use std::fmt::Write;

use crate::language::ast::{Directive, DirectiveLocation, Value};
use crate::schema::{SchemaOrigin, SemanticSchema, SemanticType, SemanticTypeRefKind, TypeKind};

pub(super) fn description(value: Option<&crate::language::ast::Description>) -> Option<&str> {
    value.map(|description| description.value.value.as_str())
}

pub(super) fn directive_string<'a>(
    directives: &'a [Directive],
    name: &str,
    argument: &str,
) -> Option<&'a str> {
    let directive = directives
        .iter()
        .find(|directive| directive.name.value == name)?;
    let value = directive
        .arguments
        .iter()
        .find(|value| value.name.value == argument)?;
    match &value.value.value {
        Value::String(value) => Some(value.value.as_str()),
        _ => None,
    }
}

pub(super) fn deprecation(directives: &[Directive]) -> (bool, Option<&str>) {
    let Some(directive) = directives
        .iter()
        .find(|directive| directive.name.value == "deprecated")
    else {
        return (false, None);
    };
    let reason = directive
        .arguments
        .iter()
        .find(|argument| argument.name.value == "reason")
        .and_then(|argument| match &argument.value.value {
            Value::String(value) => Some(value.value.as_str()),
            _ => None,
        })
        .unwrap_or("No longer supported");
    (true, Some(reason))
}

pub(super) fn serialize_default(value: &Value, output: &mut impl Write) -> std::fmt::Result {
    match value {
        Value::Variable(name) => write!(output, "${}", name.value),
        Value::Int(value) | Value::Float(value) => output.write_str(value),
        Value::String(value) => write_string(&value.value, output),
        Value::Boolean(value) => output.write_str(if *value { "true" } else { "false" }),
        Value::Null => output.write_str("null"),
        Value::Enum(name) => output.write_str(&name.value),
        Value::List(values) => {
            output.write_char('[')?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.write_str(", ")?;
                }
                serialize_default(&value.value, output)?;
            }
            output.write_char(']')
        }
        Value::Object(fields) => {
            output.write_char('{')?;
            for (index, field) in fields.iter().enumerate() {
                if index > 0 {
                    output.write_str(", ")?;
                }
                write!(output, "{}: ", field.name.value)?;
                serialize_default(&field.value.value, output)?;
            }
            output.write_char('}')
        }
    }
}

fn write_string(value: &str, output: &mut impl Write) -> std::fmt::Result {
    output.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => output.write_str("\\\"")?,
            '\\' => output.write_str("\\\\")?,
            '\n' => output.write_str("\\n")?,
            '\r' => output.write_str("\\r")?,
            '\t' => output.write_str("\\t")?,
            value if value.is_control() => write!(output, "\\u{:04X}", value as u32)?,
            value => output.write_char(value)?,
        }
    }
    output.write_char('"')
}

pub(super) fn directive_location(value: DirectiveLocation) -> &'static str {
    match value {
        DirectiveLocation::Query => "QUERY",
        DirectiveLocation::Mutation => "MUTATION",
        DirectiveLocation::Subscription => "SUBSCRIPTION",
        DirectiveLocation::Field => "FIELD",
        DirectiveLocation::FragmentDefinition => "FRAGMENT_DEFINITION",
        DirectiveLocation::FragmentSpread => "FRAGMENT_SPREAD",
        DirectiveLocation::InlineFragment => "INLINE_FRAGMENT",
        DirectiveLocation::VariableDefinition => "VARIABLE_DEFINITION",
        DirectiveLocation::Schema => "SCHEMA",
        DirectiveLocation::Scalar => "SCALAR",
        DirectiveLocation::Object => "OBJECT",
        DirectiveLocation::FieldDefinition => "FIELD_DEFINITION",
        DirectiveLocation::ArgumentDefinition => "ARGUMENT_DEFINITION",
        DirectiveLocation::Interface => "INTERFACE",
        DirectiveLocation::Union => "UNION",
        DirectiveLocation::Enum => "ENUM",
        DirectiveLocation::EnumValue => "ENUM_VALUE",
        DirectiveLocation::InputObject => "INPUT_OBJECT",
        DirectiveLocation::InputFieldDefinition => "INPUT_FIELD_DEFINITION",
    }
}

pub(super) fn exposed_types(schema: &SemanticSchema) -> Vec<&SemanticType> {
    let referenced = referenced_builtins(schema);
    schema
        .types()
        .iter()
        .filter(|ty| is_exposed_type(ty, &referenced))
        .collect()
}

pub(super) fn exposed_type_by_name<'a>(
    schema: &'a SemanticSchema,
    name: &str,
) -> Option<&'a SemanticType> {
    let referenced = referenced_builtins(schema);
    schema
        .type_by_name(name)
        .filter(|ty| is_exposed_type(ty, &referenced))
}

fn is_exposed_type(ty: &SemanticType, referenced: &[&str]) -> bool {
    ty.origins()
        .iter()
        .any(|origin| *origin != SchemaOrigin::BuiltIn)
        || referenced.iter().any(|name| *name == ty.name())
}

fn referenced_builtins(schema: &SemanticSchema) -> Vec<&str> {
    let mut names = vec!["Boolean", "String"];
    for ty in schema.types() {
        for field in ty.fields() {
            push_ast_named(&field.ty.kind, &mut names);
            for argument in &field.arguments {
                push_ast_named(&argument.ty.kind, &mut names);
            }
        }
        for field in ty.input_fields() {
            push_ast_named(&field.ty.kind, &mut names);
        }
    }
    for directive in schema.directive_definitions() {
        for argument in directive.arguments() {
            push_semantic_named(&argument.ty.kind, &mut names);
        }
    }
    names
}

fn push_ast_named<'a>(kind: &'a crate::language::ast::TypeRefKind, names: &mut Vec<&'a str>) {
    match kind {
        crate::language::ast::TypeRefKind::Named(name) => names.push(&name.value),
        crate::language::ast::TypeRefKind::List(inner)
        | crate::language::ast::TypeRefKind::NonNull(inner) => push_ast_named(&inner.kind, names),
    }
}

fn push_semantic_named<'a>(kind: &'a SemanticTypeRefKind, names: &mut Vec<&'a str>) {
    match kind {
        SemanticTypeRefKind::Named(name) => names.push(name),
        SemanticTypeRefKind::List(inner) | SemanticTypeRefKind::NonNull(inner) => {
            push_semantic_named(&inner.kind, names);
        }
    }
}

pub(super) const fn kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Scalar => "SCALAR",
        TypeKind::Object => "OBJECT",
        TypeKind::Interface => "INTERFACE",
        TypeKind::Union => "UNION",
        TypeKind::Enum => "ENUM",
        TypeKind::InputObject => "INPUT_OBJECT",
    }
}

#[cfg(test)]
mod tests {
    use super::exposed_type_by_name;
    use crate::language::parse_type_system_document;
    use crate::schema::compose_schema;
    use crate::{ExecutionLimits, SourceId};

    fn schema(source: &str) -> crate::schema::SemanticSchema {
        let parsed =
            parse_type_system_document(SourceId::new(71), source, &ExecutionLimits::unbounded());
        let output = compose_schema(&[parsed.document.unwrap()], &ExecutionLimits::unbounded());
        assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
        output.schema.unwrap()
    }

    #[test]
    fn hides_unreferenced_builtins_and_publishes_them_after_reference() {
        let string_only = schema("type Query { text: String }");
        for name in ["Int", "Float", "ID"] {
            assert!(exposed_type_by_name(&string_only, name).is_none(), "{name}");
        }
        for name in ["String", "Boolean"] {
            assert!(exposed_type_by_name(&string_only, name).is_some(), "{name}");
        }

        let with_int = schema("type Query { count: Int }");
        assert!(exposed_type_by_name(&with_int, "Int").is_some());
    }
}
