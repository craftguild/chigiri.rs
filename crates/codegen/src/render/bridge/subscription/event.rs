//! Renders one typed subscription event resolver adapter.

use chigiri_core::language::ast::FieldDefinition;
use chigiri_core::schema::SemanticType;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::super::conversion::output_expression;
use super::super::super::model::RenderModel;
use super::super::super::resolvers::{
    args_type, resolver_trait, subscription_event_type, subscription_resolve_method,
};
use super::event_name;

pub(super) fn render_event(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    field_index: usize,
    field: &FieldDefinition,
) -> Result<GeneratedItem, CodegenError> {
    let name = event_name(object_index, field_index);
    let resolver = resolver_trait(model, object.name());
    let method = subscription_resolve_method(field);
    let event = subscription_event_type(field);
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<'a, C, T, R, E>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolver: &'a R,");
        writer.line("__chigiri_mapper: &'a (dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync),");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: &'a ::chigiri::__private::ErasedSubscriptionArguments,");
        writer.line("__chigiri_event: &'a ::chigiri::__private::ErasedSubscriptionEvent,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>");
    render_bounds(&mut writer, &resolver);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("::std::boxed::Box::pin(async move {");
        writer.indented(|writer| {
            let arguments = if field.arguments.is_empty() {
                writer.line("let _ = __chigiri_arguments.downcast_ref::<()>()?;");
                String::new()
            } else {
                let args = args_type(model, object.name(), field)?;
                writer.line(&format!(
                    "let __chigiri_args = __chigiri_arguments.downcast_ref::<super::{args}>()?;"
                ));
                ", __chigiri_args".to_owned()
            };
            writer.line(&format!(
                "let __chigiri_event = __chigiri_event.downcast_ref::<<R as super::{resolver}<C, T, E>>::{event}>()?;"
            ));
            writer.line(&format!(
                "let __chigiri_output = <R as super::{resolver}<C, T, E>>::{method}(__chigiri_resolver, __chigiri_context{arguments}, __chigiri_event).await.map_err(__chigiri_mapper)?;"
            ));
            writer.line(&output_expression(model, &field.ty, "__chigiri_output")?);
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
