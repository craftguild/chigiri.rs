//! Render descriptions, directive applications, and literal metadata.

mod structure;

use chigiri_core::language::ast::{
    Description, Directive, EnumValueDefinition, FieldDefinition, InputValueDefinition, TypeRef,
    Value,
};
use chigiri_core::schema::SemanticDirectiveArgument;

use crate::error::CodegenError;
use crate::literal::{graphql_value, semantic_value};
use crate::naming::canonical_type_ref;
use crate::writer::{RustWriter, rust_string_literal};

use self::structure::type_ref_descriptor;

pub(super) fn render_const_value_items(
    stem: &str,
    value: &Value,
) -> Result<(Vec<crate::writer::GeneratedItem>, String), CodegenError> {
    structure::render_const_value_items(stem, value)
}

pub(super) fn render_field_descriptor(
    writer: &mut RustWriter,
    field: &FieldDefinition,
    arguments: &str,
    directives: &str,
) -> Result<(), CodegenError> {
    writer.line(&format!("name: {},", literal(&field.name.value)));
    writer.line(&format!(
        "description: {},",
        description(field.description.as_ref())
    ));
    writer.line(&format!("type_ref: {},", type_ref(&field.ty)?));
    writer.line(&format!(
        "type_descriptor: {},",
        type_ref_descriptor(&field.ty)?
    ));
    writer.line(&format!("arguments: {arguments},"));
    writer.line(&format!("directives: {directives},"));
    render_deprecation(writer, &field.directives)
}

pub(super) fn render_semantic_input_descriptor(
    writer: &mut RustWriter,
    argument: &SemanticDirectiveArgument,
    directives: &str,
    default_descriptor: Option<&str>,
) -> Result<(), CodegenError> {
    let source = argument.source_definition.as_ref().ok_or_else(|| {
        CodegenError::new(
            "SF4007",
            "custom directive argument must retain a source value",
        )
    })?;
    let default = argument
        .default_value
        .as_ref()
        .map(semantic_value)
        .transpose()
        .map_err(CodegenError::from_message)?;
    render_input_descriptor(writer, source, default, directives, default_descriptor)
}

pub(super) fn render_input_descriptor(
    writer: &mut RustWriter,
    value: &InputValueDefinition,
    default: Option<String>,
    directives: &str,
    default_descriptor: Option<&str>,
) -> Result<(), CodegenError> {
    writer.line(&format!("name: {},", literal(&value.name.value)));
    writer.line(&format!(
        "description: {},",
        description(value.description.as_ref())
    ));
    writer.line(&format!("type_ref: {},", type_ref(&value.ty)?));
    writer.line(&format!(
        "type_descriptor: {},",
        type_ref_descriptor(&value.ty)?
    ));
    writer.line(&format!("default_value: {},", optional_owned(default)));
    writer.line(&format!(
        "default_descriptor: {},",
        default_descriptor.map_or_else(|| "None".to_owned(), |value| format!("Some({value})"))
    ));
    writer.line(&format!("directives: {directives},"));
    render_deprecation(writer, &value.directives)
}

pub(super) fn render_syntax_input_descriptor(
    writer: &mut RustWriter,
    value: &InputValueDefinition,
    directives: &str,
    default_descriptor: Option<&str>,
) -> Result<(), CodegenError> {
    let default = value
        .default_value
        .as_ref()
        .map(graphql_value)
        .transpose()
        .map_err(CodegenError::from_message)?;
    render_input_descriptor(writer, value, default, directives, default_descriptor)
}

pub(super) fn render_enum_descriptor(
    writer: &mut RustWriter,
    value: &EnumValueDefinition,
    directives: &str,
) -> Result<(), CodegenError> {
    writer.line(&format!("name: {},", literal(&value.name.value)));
    writer.line(&format!(
        "description: {},",
        description(value.description.as_ref())
    ));
    writer.line(&format!("directives: {directives},"));
    render_deprecation(writer, &value.directives)
}

pub(super) fn description(value: Option<&Description>) -> String {
    optional_literal(value.map(|value| value.value.value.as_str()))
}

pub(super) fn specified_by_url(directives: &[Directive]) -> Result<Option<&str>, CodegenError> {
    let Some(directive) = named_directive(directives, "specifiedBy") else {
        return Ok(None);
    };
    let argument = directive
        .arguments
        .iter()
        .find(|value| value.name.value == "url")
        .ok_or_else(|| CodegenError::new("SF4007", "@specifiedBy url is missing."))?;
    match &argument.value.value {
        Value::String(value) => Ok(Some(&value.value)),
        _ => Err(CodegenError::new(
            "SF4007",
            "@specifiedBy url is not String.",
        )),
    }
}

pub(super) fn has_directive(directives: &[Directive], name: &str) -> bool {
    named_directive(directives, name).is_some()
}

pub(super) fn optional_literal(value: Option<&str>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |value| format!("Some({})", literal(value)),
    )
}

pub(super) fn literal(value: &str) -> String {
    rust_string_literal(value)
}

fn type_ref(value: &TypeRef) -> Result<String, CodegenError> {
    canonical_type_ref(value, usize::MAX)
        .map(|value| literal(&value))
        .map_err(|error| CodegenError::from_message(format!("{}: {}", error.code, error.message)))
}

fn render_deprecation(
    writer: &mut RustWriter,
    directives: &[Directive],
) -> Result<(), CodegenError> {
    let reason = optional_owned(deprecation_reason(directives)?);
    writer.line(&format!("deprecation_reason: {reason},"));
    Ok(())
}

fn deprecation_reason(directives: &[Directive]) -> Result<Option<String>, CodegenError> {
    let Some(directive) = named_directive(directives, "deprecated") else {
        return Ok(None);
    };
    let Some(argument) = directive
        .arguments
        .iter()
        .find(|value| value.name.value == "reason")
    else {
        return Ok(Some("No longer supported".to_owned()));
    };
    match &argument.value.value {
        Value::String(value) => Ok(Some(value.value.clone())),
        Value::Null => Ok(None),
        _ => Err(CodegenError::new(
            "SF4007",
            "@deprecated reason is not String or null.",
        )),
    }
}

fn named_directive<'a>(directives: &'a [Directive], name: &str) -> Option<&'a Directive> {
    directives
        .iter()
        .find(|directive| directive.name.value == name)
}

fn optional_owned(value: Option<String>) -> String {
    optional_literal(value.as_deref())
}
