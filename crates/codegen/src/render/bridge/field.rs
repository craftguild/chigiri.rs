//! Convert a schema field from typed dispatch to dynamic output.

use chigiri_core::language::ast::FieldDefinition;
use chigiri_core::schema::SemanticType;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

use super::super::conversion::output_expression;
use super::super::model::{RenderModel, field_name};
use super::super::resolvers::resolver_trait;
use super::arguments::{render_coercion, render_typed};
use super::field_function_name;

pub(super) fn render_field(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    field_index: usize,
    definition: &FieldDefinition,
) -> Result<GeneratedItem, CodegenError> {
    let name = field_function_name(object_index, field_index);
    let mut writer = RustWriter::new();
    render_header(&mut writer, model, object, &name);
    writer.indented(|writer| {
        writer.line("::std::boxed::Box::pin(async move {");
        writer.indented(|writer| render_body(writer, model, object, definition))?;
        writer.line("})");
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(GeneratedItem::new(name, writer.finish()))
}

fn render_header(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &SemanticType,
    name: &str,
) {
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a (dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync),");
        writer.line("__chigiri_context: &'a C,");
        writer.line(
            "__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,",
        );
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>");
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
    writer.line("{");
}

fn render_body(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &SemanticType,
    definition: &FieldDefinition,
) -> Result<(), CodegenError> {
    render_coercion(writer, model, definition)?;
    render_typed(writer, model, object.name(), definition)?;
    render_parent(writer, model, object);
    writer.line(&format!(
        "let __chigiri_ctx = ::chigiri::FieldContext::new(__chigiri_context, {}, {});",
        rust_string_literal(object.name()),
        rust_string_literal(&definition.name.value)
    ));
    let resolver = resolver_trait(model, object.name());
    let mut call = format!(
        "<R as super::{resolver}<C, T, E>>::{}(__chigiri_resolver, __chigiri_ctx",
        field_name(&definition.name.value)
    );
    if !model.is_root(object.name()) || model.object_requires_domain(object.name()) {
        call.push_str(", __chigiri_typed_parent");
    }
    if !definition.arguments.is_empty() {
        call.push_str(", __chigiri_args");
    }
    // Only the awaited application result is mapped. Coercion above and output
    // conversion below remain framework FieldError paths, including list items.
    call.push_str(").await.map_err(__chigiri_mapper)?");
    writer.line(&format!("let __chigiri_output = {call};"));
    writer.line(&output_expression(
        model,
        &definition.ty,
        "__chigiri_output",
    )?);
    Ok(())
}

fn render_parent(writer: &mut RustWriter, model: &RenderModel<'_>, object: &SemanticType) {
    let object_name = model.type_name(object.name());
    if model.is_subscription_root(object.name()) {
        writer.line(&format!(
            "let __chigiri_typed_parent = ::chigiri::__private::required_parent::<T::{object_name}>(__chigiri_parent, {})?;",
            rust_string_literal(object.name())
        ));
    } else if model.is_root(object.name()) {
        if model.object_requires_domain(object.name()) {
            writer.line(&format!(
                "let __chigiri_typed_parent = ::chigiri::__private::optional_parent::<T::{object_name}>(__chigiri_parent, {})?;",
                rust_string_literal(object.name())
            ));
        } else {
            writer.line("if __chigiri_parent.is_some() {");
            writer.indented(|writer| {
                writer.line("return Err(::chigiri::__private::invariant_error(\"root resolver received an unexpected parent value\"));");
            });
            writer.line("}");
        }
    } else {
        writer.line(&format!(
            "let __chigiri_typed_parent = ::chigiri::__private::required_parent::<T::{object_name}>(__chigiri_parent, {})?;",
            rust_string_literal(object.name())
        ));
    }
}
