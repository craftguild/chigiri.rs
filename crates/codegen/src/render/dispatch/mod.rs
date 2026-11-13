//! Linear typestate assembly and final storage for resolver groups.

mod stages;
mod storage;

use chigiri_core::schema::TypeKind;

use crate::writer::{GeneratedItem, RustWriter};

use super::dispatch_group::render_group;
use super::model::RenderModel;

pub(super) struct DispatchItems {
    pub(super) definitions: Vec<GeneratedItem>,
    pub(super) methods: Vec<GeneratedItem>,
}

pub(super) fn render_dispatch_items(model: &RenderModel<'_>) -> DispatchItems {
    let objects = model
        .user_types()
        .filter(|ty| ty.kind() == TypeKind::Object)
        .collect::<Vec<_>>();
    let mut definitions = objects
        .iter()
        .enumerate()
        .map(|(index, object)| {
            render_group(model, index, object).in_file(format!(
                "internal/dispatch/{}.rs",
                super::layout::owner(object.name())
            ))
        })
        .collect::<Vec<_>>();
    definitions.push(storage::render_resolvers(objects.len()).in_file("assembly.rs"));
    // Typestate columns preserve schema source order throughout resolver assembly.
    let methods = objects
        .iter()
        .enumerate()
        .map(|(index, object)| stages::render_stage(model, index, object).in_file("assembly.rs"))
        .collect();
    DispatchItems {
        definitions,
        methods,
    }
}

fn write_bounds(writer: &mut RustWriter) {
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
    });
}

fn write_stage_bounds(writer: &mut RustWriter) {
    write_bounds(writer);
    writer.indented(write_error_bound);
}

pub(super) fn write_error_bound(writer: &mut RustWriter) {
    writer.line("E: ::std::error::Error + ::std::marker::Send + ::std::marker::Sync + 'static,");
}
