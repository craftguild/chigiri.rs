//! Converts one ordinary resolver group into the common object-safe adapter.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter};

use super::super::bridge::object_router_name;
use super::super::model::RenderModel;
use super::super::resolvers::resolver_trait;

pub(in crate::render) fn render_group(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
) -> GeneratedItem {
    let rust_object = model.type_name(object.name());
    let adapter = format!("{rust_object}Adapter");
    let resolver = resolver_trait(model, object.name());
    let router = object_router_name(object_index);
    let mut writer = RustWriter::new();

    writer.line(&format!("pub(super) struct {adapter}<C, T, R, E> {{"));
    writer.indented(|writer| {
        writer.line("resolver: R,");
        writer.line("mapper: ::std::sync::Arc<dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync>,");
        writer.line("marker: ::std::marker::PhantomData<fn() -> (C, T)>,");
    });
    writer.line("}");
    writer.blank_line();
    writer.line(&format!("impl<C, T, R, E> {adapter}<C, T, R, E> {{"));
    writer.indented(|writer| {
        writer.line("pub(super) const fn new(resolver: R, mapper: ::std::sync::Arc<dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync>) -> Self {");
        writer.indented(|writer| {
            writer.line("Self {");
            writer.indented(|writer| {
                writer.line("resolver,");
                writer.line("mapper,");
                writer.line("marker: ::std::marker::PhantomData,");
            });
            writer.line("}");
        });
        writer.line("}");
    });
    writer.line("}");
    writer.blank_line();
    if model.is_subscription_root(object.name()) && !model.object_requires_domain(object.name()) {
        render_unavailable_ordinary_impl(&mut writer, model, object_index, object, &adapter);
    } else {
        render_ordinary_impl(
            &mut writer,
            model,
            object_index,
            object,
            &adapter,
            &resolver,
            &router,
        );
    }
    if model.is_subscription_root(object.name()) {
        writer.blank_line();
        super::subscription::render_impl(&mut writer, model, object_index, object, &adapter);
    }

    GeneratedItem::new(adapter, writer.finish())
}

fn render_ordinary_impl(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    adapter: &str,
    resolver: &str,
    router: &str,
) {
    writer.line(&format!(
        "impl<C, T, R, E> ::chigiri::__private::ResolverGroupDispatch<C> for {adapter}<C, T, R, E>"
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
        writer.line("fn object_index(&self) -> usize {");
        writer.indented(|writer| writer.line(&object_index.to_string()));
        writer.line("}");
        writer.blank_line();
        writer.line("fn resolve<'a>(");
        writer.indented(|writer| {
            writer.line("&'a self,");
            writer.line("__chigiri_context: &'a C,");
            writer.line("__chigiri_field_name: &'a str,");
            writer.line(
                "__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,",
            );
            writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
        });
        writer.line(
            ") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>> {",
        );
        writer.indented(|writer| {
            writer.line(&format!(
                "super::{router}::<C, T, R, E>(&self.resolver, &*self.mapper, __chigiri_context, __chigiri_field_name, __chigiri_parent, __chigiri_arguments)"
            ));
        });
        writer.line("}");
    });
    writer.line("}");
    let _ = (model, object);
}

fn render_unavailable_ordinary_impl(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
    adapter: &str,
) {
    let resolver = resolver_trait(model, object.name());
    writer.line(&format!(
        "impl<C, T, R, E> ::chigiri::__private::ResolverGroupDispatch<C> for {adapter}<C, T, R, E>"
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
        writer.line("fn object_index(&self) -> usize {");
        writer.indented(|writer| writer.line(&object_index.to_string()));
        writer.line("}");
        writer.blank_line();
        writer.line("fn resolve<'a>(");
        writer.indented(|writer| {
            writer.line("&'a self,");
            writer.line("__chigiri_context: &'a C,");
            writer.line("__chigiri_field_name: &'a str,");
            writer.line("__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,");
            writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
        });
        writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>> {");
        writer.indented(|writer| {
            writer.line("::std::boxed::Box::pin(async move {");
            writer.indented(|writer| {
                writer.line("let _ = (&self.resolver, __chigiri_context, __chigiri_field_name, __chigiri_parent, __chigiri_arguments);");
                writer.line("Err(::chigiri::__private::invariant_error(\"subscription root is not available as an ordinary nested object\"))");
            });
            writer.line("})");
        });
        writer.line("}");
    });
    writer.line("}");
}
