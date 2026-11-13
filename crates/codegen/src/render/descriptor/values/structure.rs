//! Render parse-free type references and constant-value expressions.

use chigiri_core::language::ast::{Spanned, TypeRef, TypeRefKind, Value};

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::lists::{descriptor_list_expression, render_descriptor_list};
use super::literal;

pub(super) fn type_ref_descriptor(value: &TypeRef) -> Result<String, CodegenError> {
    let descriptor = match &value.kind {
        TypeRefKind::Named(name) => {
            format!(
                "::chigiri::TypeRefDescriptor::Named({})",
                literal(&name.value)
            )
        }
        TypeRefKind::List(item) => format!(
            "::chigiri::TypeRefDescriptor::List(&{})",
            type_ref_descriptor(item)?
        ),
        TypeRefKind::NonNull(inner) => format!(
            "::chigiri::TypeRefDescriptor::NonNull(&{})",
            type_ref_descriptor(inner)?
        ),
    };
    Ok(descriptor)
}

pub(super) fn render_const_value_items(
    stem: &str,
    value: &Value,
) -> Result<(Vec<GeneratedItem>, String), CodegenError> {
    let mut items = Vec::new();
    let expression = const_value_descriptor(stem, value, &mut items)?;
    Ok((items, expression))
}

fn const_value_descriptor(
    stem: &str,
    value: &Value,
    items: &mut Vec<GeneratedItem>,
) -> Result<String, CodegenError> {
    let descriptor = match value {
        Value::Variable(_) => {
            return Err(CodegenError::new(
                "SF4007",
                "The validated const value contains a variable.",
            ));
        }
        Value::Int(value) => format!("::chigiri::ConstValueDescriptor::Int({})", literal(value)),
        Value::Float(value) => {
            format!("::chigiri::ConstValueDescriptor::Float({})", literal(value))
        }
        Value::String(value) => format!(
            "::chigiri::ConstValueDescriptor::String({})",
            literal(&value.value)
        ),
        Value::Boolean(value) => {
            format!("::chigiri::ConstValueDescriptor::Boolean({value})")
        }
        Value::Null => "::chigiri::ConstValueDescriptor::Null".to_owned(),
        Value::Enum(value) => format!(
            "::chigiri::ConstValueDescriptor::Enum({})",
            literal(&value.value)
        ),
        Value::List(values) => render_list(stem, values, items)?,
        Value::Object(fields) => render_object(stem, fields, items)?,
    };
    Ok(descriptor)
}

fn render_list(
    stem: &str,
    values: &[Spanned<Value>],
    items: &mut Vec<GeneratedItem>,
) -> Result<String, CodegenError> {
    let list_stem = format!("{stem}_ITEM");
    for (index, value) in values.iter().enumerate() {
        let name = format!("{list_stem}_{index:04}");
        let expression = const_value_descriptor(&name, &value.value, items)?;
        items.push(render_const_item(name, expression));
    }
    items.extend(render_descriptor_list(
        &list_stem,
        values.len(),
        "::chigiri::ConstValueDescriptor",
    ));
    Ok(format!(
        "::chigiri::ConstValueDescriptor::List({})",
        descriptor_list_expression(&list_stem, values.len())
    ))
}

fn render_object(
    stem: &str,
    fields: &[chigiri_core::language::ast::ObjectField],
    items: &mut Vec<GeneratedItem>,
) -> Result<String, CodegenError> {
    let field_stem = format!("{stem}_FIELD");
    for (index, field) in fields.iter().enumerate() {
        let name = format!("{field_stem}_{index:04}");
        let value = const_value_descriptor(&format!("{name}_VALUE"), &field.value.value, items)?;
        let mut writer = RustWriter::new();
        writer.line(&format!(
            "pub(super) const {name}: ::chigiri::ConstObjectFieldDescriptor = ::chigiri::ConstObjectFieldDescriptor {{"
        ));
        writer.indented(|writer| {
            writer.line(&format!("name: {},", literal(&field.name.value)));
            writer.line(&format!("value: {value},"));
        });
        writer.line("};");
        items.push(GeneratedItem::new(name, writer.finish()));
    }
    items.extend(render_descriptor_list(
        &field_stem,
        fields.len(),
        "::chigiri::ConstObjectFieldDescriptor",
    ));
    Ok(format!(
        "::chigiri::ConstValueDescriptor::Object({})",
        descriptor_list_expression(&field_stem, fields.len())
    ))
}

fn render_const_item(name: String, expression: String) -> GeneratedItem {
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) const {name}: ::chigiri::ConstValueDescriptor = {expression};"
    ));
    GeneratedItem::new(name, writer.finish())
}
