//! Splits object-field coordinate routing into fixed-width chunks.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

use super::super::model::RenderModel;
use super::super::resolvers::resolver_trait;
use super::field_function_name;
use super::object_router_name;

// Internal structure that fixes the branching and register width of a function. It is not used for file partitioning conditions.
const ROUTES_PER_CHUNK: usize = 32;

pub(super) fn render_object(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
) -> Vec<GeneratedItem> {
    let mut items = Vec::new();
    let mut routes = Vec::new();
    for (chunk_index, fields) in object.fields().chunks(ROUTES_PER_CHUNK).enumerate() {
        items.push(render_chunk(
            model,
            object_index,
            chunk_index,
            object,
            fields,
        ));
        routes.push(chunk_name(object_index, chunk_index));
    }
    let mut level = 0;
    while routes.len() > ROUTES_PER_CHUNK {
        let mut parents = Vec::new();
        for (group_index, children) in routes.chunks(ROUTES_PER_CHUNK).enumerate() {
            let name = group_name(object_index, level, group_index);
            items.push(render_route_group(model, object, &name, children));
            parents.push(name);
        }
        routes = parents;
        level += 1;
    }
    items.push(render_router(model, object_index, object, &routes));
    items
}

fn render_chunk(
    model: &RenderModel<'_>,
    object_index: usize,
    chunk_index: usize,
    object: &SemanticType,
    fields: &[chigiri_core::language::ast::FieldDefinition],
) -> GeneratedItem {
    let name = chunk_name(object_index, chunk_index);
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    render_parameters(&mut writer);
    writer.line(") -> ::chigiri::__private::DispatchAttempt<::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>>");
    render_bounds(&mut writer, model, object);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("match __chigiri_field_name {");
        writer.indented(|writer| {
            for (offset, field) in fields.iter().enumerate() {
                let field_index = chunk_index * ROUTES_PER_CHUNK + offset;
                writer.line(&format!(
                    "{} => ::chigiri::__private::DispatchAttempt::Matched(super::{}(__chigiri_resolver, __chigiri_mapper, __chigiri_context, __chigiri_parent, __chigiri_arguments)),",
                    rust_string_literal(&field.name.value),
                    field_function_name(object_index, field_index)
                ));
            }
            writer.line("_ => ::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments),");
        });
        writer.line("}");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_route_group(
    model: &RenderModel<'_>,
    object: &SemanticType,
    name: &str,
    children: &[String],
) -> GeneratedItem {
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    render_parameters(&mut writer);
    writer.line(") -> ::chigiri::__private::DispatchAttempt<::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>>");
    render_bounds(&mut writer, model, object);
    writer.line("{");
    writer.indented(|writer| render_attempts(writer, children));
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_router(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    routes: &[String],
) -> GeneratedItem {
    let name = object_router_name(object_index);
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    render_parameters(&mut writer);
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>");
    render_bounds(&mut writer, model, object);
    writer.line("{");
    writer.indented(|writer| {
        render_final_attempts(writer, routes);
        writer.line("::std::boxed::Box::pin(async move {");
        writer.indented(|writer| {
            writer.line("let _ = (__chigiri_resolver, __chigiri_mapper, __chigiri_context, __chigiri_parent, __chigiri_arguments);");
            writer.line(&format!(
                "Err(::chigiri::__private::invariant_error(format!(\"generated dispatch has no field `{}.{{__chigiri_field_name}}`\")))",
                object.name()
            ));
        });
        writer.line("})");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_attempts(writer: &mut RustWriter, routes: &[String]) {
    for route in routes {
        write_attempt(
            writer,
            route,
            "return ::chigiri::__private::DispatchAttempt::Matched(__chigiri_future)",
        );
    }
    writer.line("::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments)");
}

fn render_final_attempts(writer: &mut RustWriter, routes: &[String]) {
    for route in routes {
        write_attempt(writer, route, "return __chigiri_future");
    }
}

fn write_attempt(writer: &mut RustWriter, route: &str, matched: &str) {
    writer.line(&format!(
        "let __chigiri_arguments = match super::{route}(__chigiri_resolver, __chigiri_mapper, __chigiri_context, __chigiri_field_name, __chigiri_parent, __chigiri_arguments) {{"
    ));
    writer.indented(|writer| {
        writer.line(&format!(
            "::chigiri::__private::DispatchAttempt::Matched(__chigiri_future) => {matched},"
        ));
        writer.line("::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments) => __chigiri_arguments,");
    });
    writer.line("};");
}

fn render_parameters(writer: &mut RustWriter) {
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a (dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync),");
        writer.line("__chigiri_context: &'a C,");
        writer.line("__chigiri_field_name: &'a str,");
        writer.line(
            "__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,",
        );
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
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
        super::super::dispatch::write_error_bound(writer);
    });
}

fn chunk_name(object_index: usize, chunk_index: usize) -> String {
    format!("__chigiri_route_{object_index:04}_{chunk_index:04}")
}

fn group_name(object_index: usize, level: usize, group_index: usize) -> String {
    format!("__chigiri_route_group_{object_index:04}_{level:02}_{group_index:04}")
}
