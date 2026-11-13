//! Routes one subscription root group by the validated field coordinate.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

use super::super::super::model::RenderModel;
use super::super::super::resolvers::resolver_trait;
use super::{event_name, event_router_name, setup_name, subscribe_router_name};

pub(super) fn render_subscribe_router(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
) -> GeneratedItem {
    let name = subscribe_router_name(object_index);
    let mut writer = RustWriter::new();
    render_subscribe_header(&mut writer, model, object, &name);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("match __chigiri_context.field_name() {");
        writer.indented(|writer| {
            for (field_index, field) in object.fields().iter().enumerate() {
                writer.line(&format!(
                    "{} => super::{}(__chigiri_resolver, __chigiri_mapper, __chigiri_context, __chigiri_arguments),",
                    rust_string_literal(&field.name.value),
                    setup_name(object_index, field_index)
                ));
            }
            render_missing_subscribe(writer);
        });
        writer.line("}");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

pub(super) fn render_event_router(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
) -> GeneratedItem {
    let name = event_router_name(object_index);
    let mut writer = RustWriter::new();
    render_event_header(&mut writer, model, object, &name);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("match __chigiri_context.field_name() {");
        writer.indented(|writer| {
            for (field_index, field) in object.fields().iter().enumerate() {
                writer.line(&format!(
                    "{} => super::{}(__chigiri_resolver, __chigiri_mapper, __chigiri_context, __chigiri_arguments, __chigiri_event),",
                    rust_string_literal(&field.name.value),
                    event_name(object_index, field_index)
                ));
            }
            render_missing_event(writer);
        });
        writer.line("}");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_subscribe_header(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &SemanticType,
    name: &str,
) {
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a ::std::sync::Arc<dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync>,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ErasedSubscription>>");
    render_bounds(writer, model, object);
}

fn render_event_header(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &SemanticType,
    name: &str,
) {
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a (dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync),");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: &'a ::chigiri::__private::ErasedSubscriptionArguments,");
        writer.line("__chigiri_event: &'a ::chigiri::__private::ErasedSubscriptionEvent,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>");
    render_bounds(writer, model, object);
}

fn render_bounds(writer: &mut RustWriter, model: &RenderModel<'_>, object: &SemanticType) {
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
        writer.line(&format!(
            "R: super::{}<C, T, E>,",
            resolver_trait(model, object.name())
        ));
        super::super::super::dispatch::write_error_bound(writer);
    });
}

fn render_missing_subscribe(writer: &mut RustWriter) {
    writer.line("_ => ::std::boxed::Box::pin(async move {");
    writer.indented(|writer| {
        writer.line("let _ = (__chigiri_resolver, __chigiri_mapper, __chigiri_arguments);");
        writer.line("Err(::chigiri::__private::invariant_error(\"generated subscription dispatch has no source field\"))");
    });
    writer.line("}),");
}

fn render_missing_event(writer: &mut RustWriter) {
    writer.line("_ => ::std::boxed::Box::pin(async move {");
    writer.indented(|writer| {
        writer.line("let _ = (__chigiri_resolver, __chigiri_mapper, __chigiri_arguments, __chigiri_event);");
        writer.line("Err(::chigiri::__private::invariant_error(\"generated subscription dispatch has no event field\"))");
    });
    writer.line("}),");
}
