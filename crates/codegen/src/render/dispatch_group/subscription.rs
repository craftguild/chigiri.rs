//! Renders the subscription capability of the root-group adapter.

use chigiri_core::schema::SemanticType;

use crate::writer::RustWriter;

use super::super::bridge::subscription::{event_router_name, subscribe_router_name};
use super::super::model::RenderModel;
use super::super::resolvers::resolver_trait;

pub(super) fn render_impl(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    adapter: &str,
) {
    let resolver = resolver_trait(model, object.name());
    let subscribe = subscribe_router_name(object_index);
    let event = event_router_name(object_index);
    writer.line(&format!(
        "impl<C, T, R, E> ::chigiri::__private::SubscriptionResolverGroupDispatch<C> for {adapter}<C, T, R, E>"
    ));
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
        writer.line(&format!("R: super::{resolver}<C, T, E>,"));
        super::super::dispatch::write_error_bound(writer);
    });
    writer.line("{");
    writer.indented(|writer| {
        render_subscribe(writer, &subscribe);
        writer.blank_line();
        render_event(writer, &event);
    });
    writer.line("}");
}

fn render_subscribe(writer: &mut RustWriter, router: &str) {
    writer.line("fn subscribe<'a>(");
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ErasedSubscription>> {");
    writer.indented(|writer| {
        writer.line(&format!(
            "super::{router}::<C, T, R, E>(&self.resolver, &self.mapper, __chigiri_context, __chigiri_arguments)"
        ));
    });
    writer.line("}");
}

fn render_event(writer: &mut RustWriter, router: &str) {
    writer.line("fn resolve_event<'a>(");
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: &'a ::chigiri::__private::ErasedSubscriptionArguments,");
        writer.line("__chigiri_event: &'a ::chigiri::__private::ErasedSubscriptionEvent,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>> {");
    writer.indented(|writer| {
        writer.line(&format!(
            "super::{router}::<C, T, R, E>(&self.resolver, &*self.mapper, __chigiri_context, __chigiri_arguments, __chigiri_event)"
        ));
    });
    writer.line("}");
}
