//! Renders ordinary object resolver traits and methods.

use chigiri_core::language::ast::FieldDefinition;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::model::{RenderModel, field_name};
use super::arguments::args_type;

pub(in crate::render) fn resolver_trait(model: &RenderModel<'_>, object: &str) -> String {
    format!("{}Resolver", model.type_name(object))
}

pub(super) fn render_trait(
    model: &RenderModel<'_>,
    object: &str,
    fields: &[FieldDefinition],
) -> Result<GeneratedItem, CodegenError> {
    let trait_name = resolver_trait(model, object);
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "/// Contract for resolving every field of the GraphQL object `{object}`."
    ));
    writer.line(&format!(
        "pub trait {trait_name}<C, T, E>: ::std::marker::Send + ::std::marker::Sync + 'static"
    ));
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
        writer
            .line("E: ::std::error::Error + ::std::marker::Send + ::std::marker::Sync + 'static,");
    });
    writer.line("{");
    writer.indented(|writer| {
        for field in fields {
            render_method(writer, model, object, field)?;
        }
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(GeneratedItem::new(trait_name, writer.finish()).exported())
}

fn render_method(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &str,
    field: &FieldDefinition,
) -> Result<(), CodegenError> {
    let args = (!field.arguments.is_empty())
        .then(|| args_type(model, object, field))
        .transpose()?;
    let output = model.output_type(&field.ty)?;
    if let Some(description) = &field.description {
        writer.doc_comment(&description.value.value);
    }
    writer.line(&format!("fn {}<'a>(", field_name(&field.name.value)));
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("ctx: ::chigiri::FieldContext<'a, C>,");
        if model.is_root(object) {
            if model.object_requires_domain(object) {
                writer.line(&format!(
                    "parent: ::std::option::Option<&'a T::{}>,",
                    model.type_name(object)
                ));
            }
        } else {
            writer.line(&format!("parent: &'a T::{},", model.type_name(object)));
        }
        if let Some(args) = &args {
            writer.line(&format!("args: super::{args},"));
        }
    });
    writer.line(") -> impl ::std::future::Future<");
    writer.indented(|writer| {
        writer.line(&format!("Output = ::std::result::Result<{output}, E>,"));
    });
    writer.line("> + ::std::marker::Send + 'a;");
    Ok(())
}
