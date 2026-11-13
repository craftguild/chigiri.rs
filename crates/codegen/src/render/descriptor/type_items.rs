//! Render type and member descriptors as standalone items.

use chigiri_core::schema::{SemanticType, TypeKind};

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::model::RenderModel;
use super::item::{render_member, render_syntax_input_items};
use super::lists::{descriptor_list_expression, render_descriptor_list, render_literal_list};
use super::values::{description, has_directive, literal, optional_literal, specified_by_url};
use super::{applications, values};

pub(super) fn render_type(
    index: usize,
    ty: &SemanticType,
    model: &RenderModel<'_>,
) -> Result<GeneratedItem, CodegenError> {
    let name = format!("TYPE_DESCRIPTOR_{index:04}");
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) const {name}: ::chigiri::TypeDescriptor = ::chigiri::TypeDescriptor {{"
    ));
    writer.indented(|writer| {
        writer.line(&format!("name: {},", literal(ty.name())));
        writer.line(&format!("kind: {},", type_kind(ty.kind())));
        writer.line(&format!("description: {},", description(ty.description())));
        render_type_collections(writer, index, ty, model);
        writer.line(&format!(
            "directives: {},",
            applications::expression(
                &format!("TYPE_DESCRIPTOR_{index:04}_DIRECTIVE"),
                ty.directives()
            )
        ));
        writer.line(&format!(
            "is_one_of: {},",
            has_directive(ty.directives(), "oneOf")
        ));
        writer.line(&format!(
            "specified_by_url: {},",
            optional_literal(specified_by_url(ty.directives())?)
        ));
        Ok::<(), CodegenError>(())
    })?;
    writer.line("};");
    Ok(GeneratedItem::new(name, writer.finish()))
}

fn render_type_collections(
    writer: &mut RustWriter,
    index: usize,
    ty: &SemanticType,
    model: &RenderModel<'_>,
) {
    for (field, suffix, count) in [
        ("fields", "FIELD", ty.fields().len()),
        ("interfaces", "INTERFACE", ty.implemented_interfaces().len()),
        (
            "possible_types",
            "POSSIBLE_TYPE",
            model.possible_objects(ty).len(),
        ),
        ("enum_values", "ENUM_VALUE", ty.enum_values().len()),
        ("input_fields", "INPUT_FIELD", ty.input_fields().len()),
    ] {
        writer.line(&format!(
            "{field}: {},",
            descriptor_list_expression(&format!("TYPE_DESCRIPTOR_{index:04}_{suffix}"), count)
        ));
    }
}

pub(super) fn render_type_members(
    type_index: usize,
    ty: &SemanticType,
    model: &RenderModel<'_>,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = Vec::new();
    for (index, field) in ty.fields().iter().enumerate() {
        items.extend(render_field(type_index, index, field)?);
    }
    for (index, value) in ty.enum_values().iter().enumerate() {
        let name = format!("TYPE_DESCRIPTOR_{type_index:04}_ENUM_VALUE_{index:04}");
        let directive_stem = format!("{name}_DIRECTIVE");
        items.extend(applications::render(&directive_stem, &value.directives)?);
        let directives = applications::expression(&directive_stem, &value.directives);
        items.push(render_member(
            name,
            "::chigiri::EnumValueDescriptor",
            |writer| values::render_enum_descriptor(writer, value, &directives),
        )?);
    }
    for (index, field) in ty.input_fields().iter().enumerate() {
        let name = format!("TYPE_DESCRIPTOR_{type_index:04}_INPUT_FIELD_{index:04}");
        items.extend(render_syntax_input_items(&name, field)?);
    }
    items.extend(render_member_lists(type_index, ty));
    items.extend(render_name_lists(type_index, ty, model));
    Ok(items)
}

fn render_field(
    type_index: usize,
    index: usize,
    field: &chigiri_core::language::ast::FieldDefinition,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let name = format!("TYPE_DESCRIPTOR_{type_index:04}_FIELD_{index:04}");
    let mut items = Vec::new();
    for (argument_index, argument) in field.arguments.iter().enumerate() {
        items.extend(render_syntax_input_items(
            &format!("{name}_ARGUMENT_{argument_index:04}"),
            argument,
        )?);
    }
    items.extend(render_descriptor_list(
        &format!("{name}_ARGUMENT"),
        field.arguments.len(),
        "::chigiri::InputValueDescriptor",
    ));
    let directive_stem = format!("{name}_DIRECTIVE");
    items.extend(applications::render(&directive_stem, &field.directives)?);
    let arguments = descriptor_list_expression(&format!("{name}_ARGUMENT"), field.arguments.len());
    let directives = applications::expression(&directive_stem, &field.directives);
    items.push(render_member(
        name,
        "::chigiri::FieldDescriptor",
        |writer| values::render_field_descriptor(writer, field, &arguments, &directives),
    )?);
    Ok(items)
}

fn render_member_lists(type_index: usize, ty: &SemanticType) -> Vec<GeneratedItem> {
    let mut items = Vec::new();
    for (suffix, count, rust_type) in [
        ("FIELD", ty.fields().len(), "::chigiri::FieldDescriptor"),
        (
            "ENUM_VALUE",
            ty.enum_values().len(),
            "::chigiri::EnumValueDescriptor",
        ),
        (
            "INPUT_FIELD",
            ty.input_fields().len(),
            "::chigiri::InputValueDescriptor",
        ),
    ] {
        items.extend(render_descriptor_list(
            &format!("TYPE_DESCRIPTOR_{type_index:04}_{suffix}"),
            count,
            rust_type,
        ));
    }
    items
}

fn render_name_lists(
    type_index: usize,
    ty: &SemanticType,
    model: &RenderModel<'_>,
) -> Vec<GeneratedItem> {
    let interfaces = ty
        .implemented_interfaces()
        .iter()
        .map(|name| literal(&name.value))
        .collect::<Vec<_>>();
    let possible = model
        .possible_objects(ty)
        .iter()
        .map(|object| literal(object.name()))
        .collect::<Vec<_>>();
    let mut items = render_literal_list(
        &format!("TYPE_DESCRIPTOR_{type_index:04}_INTERFACE"),
        &interfaces,
        "&str",
    );
    items.extend(render_literal_list(
        &format!("TYPE_DESCRIPTOR_{type_index:04}_POSSIBLE_TYPE"),
        &possible,
        "&str",
    ));
    items
}

fn type_kind(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Scalar => "::chigiri::TypeDescriptorKind::Scalar",
        TypeKind::Object => "::chigiri::TypeDescriptorKind::Object",
        TypeKind::Interface => "::chigiri::TypeDescriptorKind::Interface",
        TypeKind::Union => "::chigiri::TypeDescriptorKind::Union",
        TypeKind::Enum => "::chigiri::TypeDescriptorKind::Enum",
        TypeKind::InputObject => "::chigiri::TypeDescriptorKind::InputObject",
    }
}
