//! Converts a semantic schema into a static descriptor that the runtime need not parse.

mod applications;
mod item;
mod lists;
mod type_items;
mod values;

#[cfg(test)]
mod tests;

use chigiri_core::language::ast::DirectiveLocation;
use chigiri_core::schema::{SemanticDirectiveDefinition, SemanticType};

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::model::RenderModel;
use item::render_semantic_input_items;
use lists::{descriptor_list_expression, render_descriptor_list, render_literal_list};
use type_items::{render_type, render_type_members};
use values::{description, literal, optional_literal};

pub(super) fn render_descriptor_items(
    model: &RenderModel<'_>,
    schema_digest: chigiri_core::SchemaDigest,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = Vec::new();
    let types = model.user_types().collect::<Vec<_>>();
    for (index, ty) in types.iter().enumerate() {
        let start = items.len();
        items.extend(render_type_members(index, ty, model)?);
        items.extend(applications::render(
            &format!("TYPE_DESCRIPTOR_{index:04}_DIRECTIVE"),
            ty.directives(),
        )?);
        items.push(render_type(index, ty, model)?);
        place(
            &mut items[start..],
            &format!(
                "internal/descriptor/types/{}.rs",
                super::layout::owner(ty.name())
            ),
        );
    }
    let directives = model
        .schema
        .directive_definitions()
        .iter()
        .filter(|definition| definition.source_definition().is_some())
        .collect::<Vec<_>>();
    for (index, definition) in directives.iter().enumerate() {
        items.extend(
            render_directive_definition(index, definition)?
                .into_iter()
                .map(|item| {
                    item.in_file(format!(
                        "internal/descriptor/directives/{}.rs",
                        super::layout::owner(definition.name())
                    ))
                }),
        );
    }
    let built_in_scalar_overlays = model
        .schema
        .types()
        .iter()
        .filter(|ty| ty.name_span().is_none() && !ty.directives().is_empty())
        .collect::<Vec<_>>();
    for (index, ty) in built_in_scalar_overlays.iter().enumerate() {
        items.extend(
            render_built_in_scalar_overlay(index, ty)?
                .into_iter()
                .map(|item| {
                    item.in_file(format!(
                        "internal/descriptor/overlays/{}.rs",
                        super::layout::owner(ty.name())
                    ))
                }),
        );
    }
    let aggregate = items.len();
    items.extend(render_descriptor_list(
        "TYPE_DESCRIPTOR",
        types.len(),
        "::chigiri::TypeDescriptor",
    ));
    items.extend(render_descriptor_list(
        "DIRECTIVE_DESCRIPTOR",
        directives.len(),
        "::chigiri::DirectiveDefinitionDescriptor",
    ));
    items.extend(render_descriptor_list(
        "BUILT_IN_SCALAR_OVERLAY",
        built_in_scalar_overlays.len(),
        "::chigiri::BuiltInScalarOverlayDescriptor",
    ));
    items.extend(applications::render(
        "SCHEMA_DIRECTIVE",
        model.schema.schema_directives(),
    )?);
    items.push(render_schema(
        model,
        types.len(),
        built_in_scalar_overlays.len(),
        directives.len(),
        schema_digest,
    )?);
    place(&mut items[aggregate..], "internal/descriptor/schema.rs");
    Ok(items)
}

fn place(items: &mut [GeneratedItem], path: &str) {
    for item in items {
        item.set_file(path);
    }
}

fn render_directive_definition(
    index: usize,
    definition: &SemanticDirectiveDefinition,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let name = format!("DIRECTIVE_DESCRIPTOR_{index:04}");
    definition
        .source_definition()
        .ok_or_else(|| CodegenError::new("SF4007", "custom directive has no source definition"))?;
    let mut items = Vec::new();
    for (argument_index, argument) in definition.arguments().iter().enumerate() {
        let argument_name = format!("{name}_ARGUMENT_{argument_index:04}");
        items.extend(render_semantic_input_items(&argument_name, argument)?);
    }
    items.extend(render_descriptor_list(
        &format!("{name}_ARGUMENT"),
        definition.arguments().len(),
        "::chigiri::InputValueDescriptor",
    ));
    let locations = definition
        .locations()
        .iter()
        .map(|location| literal(directive_location(location.value)))
        .collect::<Vec<_>>();
    items.extend(render_literal_list(
        &format!("{name}_LOCATION"),
        &locations,
        "&str",
    ));
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) const {name}: ::chigiri::DirectiveDefinitionDescriptor = ::chigiri::DirectiveDefinitionDescriptor {{"
    ));
    writer.indented(|writer| {
        writer.line(&format!("name: {},", literal(definition.name())));
        writer.line(&format!(
            "description: {},",
            description(definition.description())
        ));
        writer.line(&format!(
            "arguments: {},",
            descriptor_list_expression(&format!("{name}_ARGUMENT"), definition.arguments().len())
        ));
        writer.line(&format!("repeatable: {},", definition.is_repeatable()));
        writer.line(&format!(
            "locations: {},",
            descriptor_list_expression(&format!("{name}_LOCATION"), locations.len())
        ));
        Ok::<(), CodegenError>(())
    })?;
    writer.line("};");
    items.push(GeneratedItem::new(name, writer.finish()));
    Ok(items)
}

fn render_built_in_scalar_overlay(
    index: usize,
    ty: &SemanticType,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let name = format!("BUILT_IN_SCALAR_OVERLAY_{index:04}");
    let directive_stem = format!("{name}_DIRECTIVE");
    let mut items = applications::render(&directive_stem, ty.directives())?;
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) const {name}: ::chigiri::BuiltInScalarOverlayDescriptor = ::chigiri::BuiltInScalarOverlayDescriptor {{"
    ));
    writer.indented(|writer| {
        writer.line(&format!("name: {},", literal(ty.name())));
        writer.line(&format!(
            "directives: {},",
            applications::expression(&directive_stem, ty.directives())
        ));
    });
    writer.line("};");
    items.push(GeneratedItem::new(name, writer.finish()));
    Ok(items)
}

fn render_schema(
    model: &RenderModel<'_>,
    type_count: usize,
    built_in_scalar_overlay_count: usize,
    directive_count: usize,
    schema_digest: chigiri_core::SchemaDigest,
) -> Result<GeneratedItem, CodegenError> {
    let roots = model.schema.roots();
    let query = roots
        .query()
        .ok_or_else(|| CodegenError::new("SF4007", "A schema descriptor requires a query root."))?;
    let mut writer = RustWriter::new();
    writer.line("/// Static schema descriptor that requires no runtime SDL parsing.");
    writer.line("pub(super) static GENERATED_SCHEMA_DESCRIPTOR: ::chigiri::SchemaDescriptor = ::chigiri::SchemaDescriptor {");
    writer.indented(|writer| {
        writer.line("abi_version: ::chigiri::GENERATED_ABI_VERSION,");
        writer.line(&format!(
            "schema_digest: ::chigiri::SchemaDigest::from_bytes({:?}),",
            schema_digest.as_bytes()
        ));
        writer.line(&format!(
            "description: {},",
            description(model.schema.description())
        ));
        writer.line(&format!(
            "schema_directives: {},",
            applications::expression("SCHEMA_DIRECTIVE", model.schema.schema_directives())
        ));
        writer.line("roots: ::chigiri::OperationRootsDescriptor {");
        writer.indented(|writer| {
            writer.line(&format!("query: {},", literal(&query.type_name)));
            writer.line(&format!(
                "mutation: {},",
                optional_literal(roots.mutation().map(|root| root.type_name.as_str()))
            ));
            writer.line(&format!(
                "subscription: {},",
                optional_literal(roots.subscription().map(|root| root.type_name.as_str()))
            ));
        });
        writer.line("},");
        writer.line(&format!(
            "types: {},",
            descriptor_list_expression("TYPE_DESCRIPTOR", type_count)
        ));
        writer.line(&format!(
            "built_in_scalar_overlays: {},",
            descriptor_list_expression("BUILT_IN_SCALAR_OVERLAY", built_in_scalar_overlay_count)
        ));
        writer.line(&format!(
            "directives: {},",
            descriptor_list_expression("DIRECTIVE_DESCRIPTOR", directive_count)
        ));
        Ok::<(), CodegenError>(())
    })?;
    writer.line("};");
    Ok(GeneratedItem::new(
        "GENERATED_SCHEMA_DESCRIPTOR",
        writer.finish(),
    ))
}

fn directive_location(location: DirectiveLocation) -> &'static str {
    match location {
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
