//! Render a validated schema into a stable set of generated Rust files.

mod bridge;
mod budget;
mod conversion;
mod descriptor;
mod dispatch;
mod dispatch_group;
mod layout;
pub(crate) mod model;
mod resolvers;
mod scaffold;
mod types;

#[cfg(test)]
mod tests_layout;

use crate::compile::CompiledSchema;
use crate::config::ResolvedBindings;
use crate::error::CodegenError;
use crate::formatter::format_items;
use crate::provenance::schema_content_digest;
use crate::writer::{GeneratedFile, GeneratedItem, assemble_items};

use self::bridge::render_bridge_items;
use self::budget::{RenderBudget, preflight, validate_rendered_files};
use self::conversion::render_conversion_items;
use self::descriptor::render_descriptor_items;
use self::dispatch::render_dispatch_items;
use self::model::RenderModel;
use self::resolvers::render_resolver_items;
use self::scaffold::render_scaffold;
use self::types::render_type_items;

pub(crate) fn render_schema(
    compiled: &CompiledSchema,
    bindings: &ResolvedBindings,
) -> Result<Vec<GeneratedFile>, CodegenError> {
    preflight(compiled)?;
    let model = RenderModel::build_with_bindings(&compiled.schema, &compiled.source_map, bindings)?;
    layout::validate(&model)?;
    let schema_fingerprint =
        chigiri_core::executor::generated_resolver_abi_fingerprint(&compiled.schema);
    let resolver_abi_fingerprint = bindings.resolver_abi_fingerprint(schema_fingerprint);
    let schema_digest = schema_content_digest(&compiled.sources);
    let mut items = Vec::new();
    let mut budget = RenderBudget::default();
    append(&mut items, &mut budget, render_type_items(&model)?)?;
    append(&mut items, &mut budget, render_conversion_items(&model)?)?;
    append(&mut items, &mut budget, render_resolver_items(&model)?)?;
    let dispatch = render_dispatch_items(&model);
    append(&mut items, &mut budget, dispatch.definitions)?;
    append(&mut items, &mut budget, dispatch.methods)?;
    append(
        &mut items,
        &mut budget,
        render_bridge_items(&model, resolver_abi_fingerprint)?,
    )?;
    append(
        &mut items,
        &mut budget,
        render_descriptor_items(&model, schema_digest)?,
    )?;
    let mut files = assemble_items(format_items(items)?)
        .map_err(|error| CodegenError::new(error.code.as_str(), error.message))?;
    files.extend(render_scaffold(
        compiled,
        &files,
        bindings,
        resolver_abi_fingerprint,
        schema_digest,
    )?);
    for file in &mut files {
        if file.name != "mod.rs" && !file.name.starts_with("__chigiri/") {
            file.name = format!("__chigiri/{}", file.name);
        }
    }
    let mut files = format_files(files)?;
    validate_rendered_files(&files)?;
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

fn format_files(files: Vec<GeneratedFile>) -> Result<Vec<GeneratedFile>, CodegenError> {
    // The final format of the completed file unit, including the aggregate part such as module registration.
    let items = files
        .into_iter()
        .map(|file| {
            let item = crate::writer::GeneratedItem::new(file.name, file.contents);
            if file.exports_public_items {
                item.exported()
            } else {
                item
            }
        })
        .collect();
    Ok(format_items(items)?
        .into_iter()
        .map(|item| {
            let (name, contents, exports_public_items) = item.into_parts();
            GeneratedFile {
                name,
                contents,
                exports_public_items,
            }
        })
        .collect())
}

fn append(
    items: &mut Vec<GeneratedItem>,
    budget: &mut RenderBudget,
    next: Vec<GeneratedItem>,
) -> Result<(), CodegenError> {
    budget.reserve(&next)?;
    items.extend(next);
    Ok(())
}
