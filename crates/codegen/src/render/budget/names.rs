//! Validate generated-source byte budgets for GraphQL names.

use chigiri_core::schema::SemanticSchema;

use crate::error::CodegenError;

const MAX_GRAPHQL_NAME_BYTES: usize = 255;

pub(super) fn validate(schema: &SemanticSchema) -> Result<(), CodegenError> {
    for ty in schema.types() {
        validate_name(ty.name(), "type")?;
        for interface in ty.implemented_interfaces() {
            validate_name(&interface.value, "implemented interface")?;
        }
        for member in ty.union_members() {
            validate_name(&member.value, "union member")?;
        }
        for field in ty.fields() {
            validate_name(&field.name.value, "field")?;
            for argument in &field.arguments {
                validate_name(&argument.name.value, "field argument")?;
            }
        }
        for field in ty.input_fields() {
            validate_name(&field.name.value, "input field")?;
        }
        for value in ty.enum_values() {
            validate_name(&value.name.value, "enum value")?;
        }
    }
    for directive in schema.directive_definitions() {
        validate_name(directive.name(), "directive")?;
        for argument in directive.arguments() {
            validate_name(&argument.name, "directive argument")?;
        }
    }
    Ok(())
}

fn validate_name(name: &str, element: &str) -> Result<(), CodegenError> {
    if name.len() <= MAX_GRAPHQL_NAME_BYTES {
        return Ok(());
    }
    Err(CodegenError::new(
        "SF4010",
        format!(
            "{element} GraphQL name exceeds the codegen limit: bytes={}, max={MAX_GRAPHQL_NAME_BYTES}",
            name.len()
        ),
    ))
}
