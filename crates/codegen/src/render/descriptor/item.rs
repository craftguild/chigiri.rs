//! Render field and input descriptors with their directive collections as independent items.

use chigiri_core::language::ast::InputValueDefinition;
use chigiri_core::schema::SemanticDirectiveArgument;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::{applications, values};

pub(super) fn render_member(
    name: String,
    rust_type: &str,
    body: impl FnOnce(&mut RustWriter) -> Result<(), CodegenError>,
) -> Result<GeneratedItem, CodegenError> {
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) const {name}: {rust_type} = {rust_type} {{"
    ));
    writer.indented(body)?;
    writer.line("};");
    Ok(GeneratedItem::new(name, writer.finish()))
}

pub(super) fn render_syntax_input_items(
    name: &str,
    value: &InputValueDefinition,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let directive_stem = format!("{name}_DIRECTIVE");
    let mut items = applications::render(&directive_stem, &value.directives)?;
    let default_descriptor = if let Some(default) = &value.default_value {
        let (default_items, expression) =
            values::render_const_value_items(&format!("{name}_DEFAULT"), &default.value)?;
        items.extend(default_items);
        Some(expression)
    } else {
        None
    };
    let directives = applications::expression(&directive_stem, &value.directives);
    items.push(render_member(
        name.to_owned(),
        "::chigiri::InputValueDescriptor",
        |writer| {
            values::render_syntax_input_descriptor(
                writer,
                value,
                &directives,
                default_descriptor.as_deref(),
            )
        },
    )?);
    Ok(items)
}

pub(super) fn render_semantic_input_items(
    name: &str,
    value: &SemanticDirectiveArgument,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let source = value.source_definition.as_ref().ok_or_else(|| {
        CodegenError::new(
            "SF4007",
            "custom directive argument must retain a source value",
        )
    })?;
    let directive_stem = format!("{name}_DIRECTIVE");
    let mut items = applications::render(&directive_stem, &source.directives)?;
    let default_descriptor = if let Some(default) = &source.default_value {
        let (default_items, expression) =
            values::render_const_value_items(&format!("{name}_DEFAULT"), &default.value)?;
        items.extend(default_items);
        Some(expression)
    } else {
        None
    };
    let directives = applications::expression(&directive_stem, &source.directives);
    items.push(render_member(
        name.to_owned(),
        "::chigiri::InputValueDescriptor",
        |writer| {
            values::render_semantic_input_descriptor(
                writer,
                value,
                &directives,
                default_descriptor.as_deref(),
            )
        },
    )?);
    Ok(items)
}
