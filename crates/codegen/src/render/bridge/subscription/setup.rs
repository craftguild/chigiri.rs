//! Renders one typed subscription source initializer adapter.

use chigiri_core::language::ast::FieldDefinition;
use chigiri_core::schema::SemanticType;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::super::model::RenderModel;
use super::super::super::resolvers::{resolver_trait, subscription_subscribe_method};
use super::super::arguments::{render_coercion, render_typed};
use super::setup_name;

pub(super) fn render_setup(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    field_index: usize,
    field: &FieldDefinition,
) -> Result<GeneratedItem, CodegenError> {
    let name = setup_name(object_index, field_index);
    let resolver = resolver_trait(model, object.name());
    let method = subscription_subscribe_method(field);
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a ::std::sync::Arc<dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync>,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ErasedSubscription>>");
    render_bounds(&mut writer, &resolver);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("::std::boxed::Box::pin(async move {");
        writer.indented(|writer| {
            render_coercion(writer, model, field)?;
            render_typed(writer, model, object.name(), field)?;
            let arguments = if field.arguments.is_empty() {
                ""
            } else {
                ", &__chigiri_args"
            };
            writer.line(&format!(
                "let __chigiri_source = <R as super::{resolver}<C, T, E>>::{method}(__chigiri_resolver, __chigiri_context{arguments}).await.map_err(|__chigiri_error| (__chigiri_mapper)(__chigiri_error))?;"
            ));
            let owned_arguments = if field.arguments.is_empty() {
                "()"
            } else {
                "__chigiri_args"
            };
            writer.line(&format!(
                "Ok(::chigiri::__private::erase_subscription_source(__chigiri_source, {owned_arguments}, ::std::sync::Arc::clone(__chigiri_mapper)))"
            ));
            Ok::<(), CodegenError>(())
        })?;
        writer.line("})");
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(GeneratedItem::new(name, writer.finish()))
}

fn render_bounds(writer: &mut RustWriter, resolver: &str) {
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
        writer.line(&format!("R: super::{resolver}<C, T, E>,"));
        super::super::super::dispatch::write_error_bound(writer);
    });
}
