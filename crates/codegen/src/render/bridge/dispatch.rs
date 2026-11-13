//! Renders the schema-wide generated dispatch implementation.

use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

pub(super) fn render_dispatch_impl(
    scalar_registration: Option<&str>,
    schema_route: &str,
    subscription_root: Option<&str>,
    fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
    resolver_abi_fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
) -> GeneratedItem {
    let mut writer = RustWriter::new();
    writer.line("#[doc(hidden)]");
    writer.line("impl<C, T> ::chigiri::__private::ResolverDispatch<C> for super::Resolvers<C, T>");
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
    });
    writer.line("{");
    writer.indented(|writer| {
        render_fingerprint(writer, fingerprint);
        writer.blank_line();
        render_resolver_abi_fingerprint(writer, resolver_abi_fingerprint);
        writer.blank_line();
        render_scalar_registration(writer, scalar_registration);
        writer.blank_line();
        render_resolve_method(writer, schema_route);
    });
    writer.line("}");
    writer.blank_line();
    render_subscription_dispatch(&mut writer, subscription_root);
    GeneratedItem::new("ResolverDispatch", writer.finish())
}

fn render_subscription_dispatch(writer: &mut RustWriter, subscription_root: Option<&str>) {
    writer.line("#[doc(hidden)]");
    writer.line("impl<C, T> ::chigiri::__private::SubscriptionResolverDispatch<C> for super::Resolvers<C, T>");
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
    });
    writer.line("{");
    writer.indented(|writer| {
        render_subscription_root(writer, subscription_root);
        writer.blank_line();
        render_subscribe(writer);
        writer.blank_line();
        render_event(writer);
    });
    writer.line("}");
}

fn render_subscription_root(writer: &mut RustWriter, root: Option<&str>) {
    writer.line("fn subscription_root(&self) -> ::std::option::Option<&'static str> {");
    writer.indented(|writer| match root {
        Some(root) => writer.line(&format!(
            "::std::option::Option::Some({})",
            rust_string_literal(root)
        )),
        None => writer.line("::std::option::Option::None"),
    });
    writer.line("}");
}

fn render_subscribe(writer: &mut RustWriter) {
    writer.line("fn subscribe<'a>(");
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ErasedSubscription>> {");
    writer.indented(|writer| {
        writer.line("match &self.subscription {");
        writer.indented(|writer| {
            writer.line("::std::option::Option::Some(__chigiri_subscription) => __chigiri_subscription.subscribe(__chigiri_context, __chigiri_arguments),");
            writer.line("::std::option::Option::None => ::std::boxed::Box::pin(async move {");
            writer.indented(|writer| {
                writer.line("let _ = (__chigiri_context, __chigiri_arguments);");
                writer.line("Err(::chigiri::__private::invariant_error(\"generated schema has no subscription resolver group\"))");
            });
            writer.line("}),");
        });
        writer.line("}");
    });
    writer.line("}");
}

fn render_event(writer: &mut RustWriter) {
    writer.line("fn resolve_event<'a>(");
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("__chigiri_context: ::chigiri::FieldContext<'a, C>,");
        writer.line("__chigiri_arguments: &'a ::chigiri::__private::ErasedSubscriptionArguments,");
        writer.line("__chigiri_event: &'a ::chigiri::__private::ErasedSubscriptionEvent,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>> {");
    writer.indented(|writer| {
        writer.line("match &self.subscription {");
        writer.indented(|writer| {
            writer.line("::std::option::Option::Some(__chigiri_subscription) => __chigiri_subscription.resolve_event(__chigiri_context, __chigiri_arguments, __chigiri_event),");
            writer.line("::std::option::Option::None => ::std::boxed::Box::pin(async move {");
            writer.indented(|writer| {
                writer.line("let _ = (__chigiri_context, __chigiri_arguments, __chigiri_event);");
                writer.line("Err(::chigiri::__private::invariant_error(\"generated schema has no subscription resolver group\"))");
            });
            writer.line("}),");
        });
        writer.line("}");
    });
    writer.line("}");
}

fn render_resolver_abi_fingerprint(
    writer: &mut RustWriter,
    fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
) {
    writer.line(
        "fn resolver_abi_fingerprint(&self) -> ::chigiri::__private::ResolverSchemaFingerprint {",
    );
    writer.indented(|writer| {
        writer.line(&format!(
            "::chigiri::__private::ResolverSchemaFingerprint::new({}, {}_u64)",
            fingerprint.component_count(),
            fingerprint.hash()
        ));
    });
    writer.line("}");
}

fn render_fingerprint(
    writer: &mut RustWriter,
    fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
) {
    writer
        .line("fn schema_fingerprint(&self) -> ::chigiri::__private::ResolverSchemaFingerprint {");
    writer.indented(|writer| {
        writer.line(&format!(
            "::chigiri::__private::ResolverSchemaFingerprint::new({}, {}_u64)",
            fingerprint.component_count(),
            fingerprint.hash()
        ));
    });
    writer.line("}");
}

fn render_scalar_registration(writer: &mut RustWriter, registration: Option<&str>) {
    writer.line("fn register_custom_scalars(&self, registry: &mut ::chigiri::core::executable::CustomScalarRegistry) {");
    writer.indented(|writer| {
        writer.line("let _ = (self, &mut *registry);");
        if let Some(registration) = registration {
            writer.line(&format!("super::{registration}::<T>(registry);"));
        }
    });
    writer.line("}");
}

fn render_resolve_method(writer: &mut RustWriter, schema_route: &str) {
    writer.line("fn resolve<'a>(");
    writer.indented(|writer| {
        writer.line("&'a self,");
        writer.line("__chigiri_context: &'a C,");
        writer.line("__chigiri_parent_type: &'a str,");
        writer.line("__chigiri_field_name: &'a str,");
        writer.line(
            "__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,",
        );
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>> {");
    writer.indented(|writer| {
        writer.line(&format!(
            "match super::{schema_route}(self, __chigiri_context, __chigiri_parent_type, __chigiri_field_name, __chigiri_parent, __chigiri_arguments) {{"
        ));
        writer.indented(|writer| {
            writer.line("::chigiri::__private::DispatchAttempt::Matched(__chigiri_future) => __chigiri_future,");
            writer.line("::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments) => ::std::boxed::Box::pin(async move {");
            writer.indented(|writer| {
                writer.line("let _ = (__chigiri_context, __chigiri_parent, __chigiri_arguments);");
                writer.line("Err(::chigiri::__private::invariant_error(format!(\"generated dispatch has no object `{__chigiri_parent_type}`\")))");
            });
            writer.line("}),");
        });
        writer.line("}");
    });
    writer.line("}");
}
