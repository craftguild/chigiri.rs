use crate::language::ast::Name;
use crate::{Diagnostic, Span};

use super::super::{SemanticSchema, SemanticType, TypeKind};
use super::{
    ResolvedSchemaCoordinate, SCHEMA_COORDINATE_CONTAINER_MISSING,
    SCHEMA_COORDINATE_INVALID_CONTAINER, SchemaCoordinate,
};

/// Resolves an exact coordinate against the semantic schema.
///
/// Returns `Ok(None)` when the final coordinate element is unknown. Returns `Err` when an
/// intermediate element is missing or has an incompatible container kind.
pub fn resolve_schema_coordinate<'schema>(
    schema: &'schema SemanticSchema,
    coordinate: &SchemaCoordinate,
) -> Result<Option<ResolvedSchemaCoordinate<'schema>>, Diagnostic> {
    match coordinate {
        SchemaCoordinate::Type { type_name } => Ok(schema
            .type_by_name(&type_name.value)
            .map(ResolvedSchemaCoordinate::Type)),
        SchemaCoordinate::Member {
            type_name,
            member_name,
        } => resolve_member(schema, type_name, member_name),
        SchemaCoordinate::FieldArgument {
            type_name,
            field_name,
            argument_name,
        } => resolve_field_argument(schema, type_name, field_name, argument_name),
        SchemaCoordinate::Directive { directive_name } => Ok(schema
            .directive_definition(&directive_name.value)
            .map(ResolvedSchemaCoordinate::Directive)),
        SchemaCoordinate::DirectiveArgument {
            directive_name,
            argument_name,
        } => resolve_directive_argument(schema, directive_name, argument_name),
    }
}

fn resolve_member<'schema>(
    schema: &'schema SemanticSchema,
    type_name: &Name,
    member_name: &Name,
) -> Result<Option<ResolvedSchemaCoordinate<'schema>>, Diagnostic> {
    let container = require_type(schema, type_name)?;
    let resolved = match container.kind() {
        TypeKind::Object | TypeKind::Interface => container
            .field(&member_name.value)
            .map(ResolvedSchemaCoordinate::Field),
        TypeKind::InputObject => container
            .input_field(&member_name.value)
            .map(ResolvedSchemaCoordinate::InputField),
        TypeKind::Enum => container
            .enum_value(&member_name.value)
            .map(ResolvedSchemaCoordinate::EnumValue),
        TypeKind::Scalar | TypeKind::Union => {
            return Err(invalid_container(
                type_name,
                "member coordinates require Object, Interface, Input Object, or Enum",
            ));
        }
    };
    Ok(resolved)
}

fn resolve_field_argument<'schema>(
    schema: &'schema SemanticSchema,
    type_name: &Name,
    field_name: &Name,
    argument_name: &Name,
) -> Result<Option<ResolvedSchemaCoordinate<'schema>>, Diagnostic> {
    let container = require_type(schema, type_name)?;
    if !matches!(container.kind(), TypeKind::Object | TypeKind::Interface) {
        return Err(invalid_container(
            type_name,
            "field argument coordinates require Object or Interface",
        ));
    }
    container.field(&field_name.value).ok_or_else(|| {
        missing_container(
            field_name.span,
            format!(
                "field '{}.{}' does not exist",
                type_name.value, field_name.value
            ),
        )
    })?;
    Ok(container
        .field_argument(&field_name.value, &argument_name.value)
        .map(ResolvedSchemaCoordinate::FieldArgument))
}

fn resolve_directive_argument<'schema>(
    schema: &'schema SemanticSchema,
    directive_name: &Name,
    argument_name: &Name,
) -> Result<Option<ResolvedSchemaCoordinate<'schema>>, Diagnostic> {
    let directive = schema
        .directive_definition(&directive_name.value)
        .ok_or_else(|| {
            missing_container(
                directive_name.span,
                format!("directive '@{}' does not exist", directive_name.value),
            )
        })?;
    Ok(directive
        .argument(&argument_name.value)
        .map(ResolvedSchemaCoordinate::DirectiveArgument))
}

fn require_type<'schema>(
    schema: &'schema SemanticSchema,
    type_name: &Name,
) -> Result<&'schema SemanticType, Diagnostic> {
    schema.type_by_name(&type_name.value).ok_or_else(|| {
        missing_container(
            type_name.span,
            format!("type '{}' does not exist", type_name.value),
        )
    })
}

fn missing_container(span: Span, message: String) -> Diagnostic {
    Diagnostic::new(SCHEMA_COORDINATE_CONTAINER_MISSING, message, span)
}

fn invalid_container(name: &Name, message: &'static str) -> Diagnostic {
    Diagnostic::new(
        SCHEMA_COORDINATE_INVALID_CONTAINER,
        format!("type '{}': {message}", name.value),
        name.span,
    )
}
