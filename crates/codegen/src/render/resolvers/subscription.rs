//! Renders complete subscription source, event, and nested ordinary methods.

use chigiri_core::language::ast::FieldDefinition;

use crate::error::CodegenError;
use crate::naming::prefixed_field_method;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::model::{RenderModel, field_name, variant_name};
use super::args_type;

pub(in crate::render) fn subscription_event_type(field: &FieldDefinition) -> String {
    format!("{}Event", variant_name(&field.name.value))
}

pub(in crate::render) fn subscription_subscribe_method(field: &FieldDefinition) -> String {
    prefixed_field_method("subscribe_", &field.name.value)
}

pub(in crate::render) fn subscription_resolve_method(field: &FieldDefinition) -> String {
    format!(
        "{}_event",
        prefixed_field_method("resolve_", &field.name.value)
    )
}

pub(super) fn render_trait(
    model: &RenderModel<'_>,
    object: &str,
    fields: &[FieldDefinition],
) -> Result<GeneratedItem, CodegenError> {
    let trait_name = super::resolver_trait(model, object);
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "/// Complete source and event contract for the GraphQL subscription root `{object}`."
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
            render_subscription_field(writer, model, object, field)?;
        }
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(GeneratedItem::new(trait_name, writer.finish()).exported())
}

fn render_subscription_field(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &str,
    field: &FieldDefinition,
) -> Result<(), CodegenError> {
    let event = subscription_event_type(field);
    let args = (!field.arguments.is_empty())
        .then(|| args_type(model, object, field))
        .transpose()?;
    writer.line(&format!(
        "type {event}: ::std::marker::Send + ::std::marker::Sync + 'static;"
    ));
    writer.blank_line();
    render_subscribe_method(writer, field, &event, args.as_deref());
    writer.blank_line();
    render_event_method(writer, model, field, &event, args.as_deref())?;
    if model.object_requires_domain(object) {
        writer.blank_line();
        render_ordinary_method(writer, model, object, field, args.as_deref())?;
    }
    Ok(())
}

fn render_subscribe_method(
    writer: &mut RustWriter,
    field: &FieldDefinition,
    event: &str,
    args: Option<&str>,
) {
    writer.line(&format!("fn {}<'a>(", subscription_subscribe_method(field)));
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("ctx: ::chigiri::FieldContext<'a, C>,");
        if let Some(args) = args {
            writer.line(&format!("args: &'a super::{args},"));
        }
    });
    writer.line(") -> impl ::std::future::Future<");
    writer.indented(|writer| {
        writer.line(&format!(
            "Output = ::std::result::Result<::chigiri::SubscriptionSource<Self::{event}, E>, E>,"
        ));
    });
    writer.line("> + ::std::marker::Send + 'a;");
}

fn render_event_method(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    field: &FieldDefinition,
    event: &str,
    args: Option<&str>,
) -> Result<(), CodegenError> {
    let output = model.output_type(&field.ty)?;
    writer.line(&format!("fn {}<'a>(", subscription_resolve_method(field)));
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("ctx: ::chigiri::FieldContext<'a, C>,");
        if let Some(args) = args {
            writer.line(&format!("args: &'a super::{args},"));
        }
        writer.line(&format!("event: &'a Self::{event},"));
    });
    writer.line(") -> impl ::std::future::Future<");
    writer.indented(|writer| {
        writer.line(&format!("Output = ::std::result::Result<{output}, E>,"));
    });
    writer.line("> + ::std::marker::Send + 'a;");
    Ok(())
}

fn render_ordinary_method(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &str,
    field: &FieldDefinition,
    args: Option<&str>,
) -> Result<(), CodegenError> {
    let output = model.output_type(&field.ty)?;
    if let Some(description) = &field.description {
        writer.doc_comment(&description.value.value);
    }
    writer.line(&format!("fn {}<'a>(", field_name(&field.name.value)));
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("ctx: ::chigiri::FieldContext<'a, C>,");
        writer.line(&format!("parent: &'a T::{},", model.type_name(object)));
        if let Some(args) = args {
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
