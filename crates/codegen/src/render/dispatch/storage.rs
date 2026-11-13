//! Renders final resolver storage and the typestate endpoints.

use crate::writer::{GeneratedItem, RustWriter};

pub(super) fn render_resolvers(object_count: usize) -> GeneratedItem {
    let mut writer = RustWriter::new();
    writer.line("/// Keep all the object resolver groups in the schema in source order.");
    writer.line("pub struct Resolvers<C, T>");
    super::write_bounds(&mut writer);
    writer.line("{");
    writer.indented(|writer| {
        writer.line(
            "pub(super) groups: ::std::boxed::Box<[::std::sync::Arc<dyn ::chigiri::__private::ResolverGroupDispatch<C>>]>,",
        );
        writer.line("pub(super) subscription: ::std::option::Option<::std::sync::Arc<dyn ::chigiri::__private::SubscriptionResolverGroupDispatch<C>>>,");
        writer.line("pub(super) marker: ::std::marker::PhantomData<fn() -> T>,");
    });
    writer.line("}");
    writer.blank_line();
    writer.line("#[doc(hidden)]");
    writer.line(
        "#[must_use = \"resolver assembly is incomplete; call the generated with_* method\"]",
    );
    writer.line("pub struct ResolverStage<C, T, E, const INDEX: usize>");
    super::write_stage_bounds(&mut writer);
    writer.line("{");
    writer.indented(|writer| {
        writer.line(
            "pub(super) groups: ::std::vec::Vec<::std::sync::Arc<dyn ::chigiri::__private::ResolverGroupDispatch<C>>>,",
        );
        writer.line("pub(super) subscription: ::std::option::Option<::std::sync::Arc<dyn ::chigiri::__private::SubscriptionResolverGroupDispatch<C>>>,");
        writer.line("pub(super) marker: ::std::marker::PhantomData<fn() -> T>,");
        writer.line("pub(super) mapper: ::std::sync::Arc<dyn ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync>,");
    });
    writer.line("}");
    writer.blank_line();
    writer.line("impl<C, T> Resolvers<C, T>");
    super::write_bounds(&mut writer);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("/// Create a stage that accepts only the first object resolver.");
        writer.line("/// The shared mapper converts only errors returned by application resolvers.");
        writer.line("pub fn builder<E, F>(mapper: F) -> ResolverStage<C, T, E, 0>");
        writer.line("where");
        writer.indented(|writer| {
            super::write_error_bound(writer);
            writer.line("F: ::std::ops::Fn(E) -> ::chigiri::FieldError + ::std::marker::Send + ::std::marker::Sync + 'static,");
        });
        writer.line("{");
        writer.indented(|writer| {
            writer.line("ResolverStage {");
            writer.indented(|writer| {
                writer.line(&format!(
                    "groups: ::std::vec::Vec::with_capacity({object_count}),"
                ));
                writer.line("subscription: ::std::option::Option::None,");
                writer.line("marker: ::std::marker::PhantomData,");
                writer.line("mapper: ::std::sync::Arc::new(mapper),");
            });
            writer.line("}");
        });
        writer.line("}");
    });
    writer.line("}");
    writer.blank_line();
    writer.line(&format!(
        "impl<C, T, E> ResolverStage<C, T, E, {object_count}>"
    ));
    super::write_stage_bounds(&mut writer);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("/// Convert the complete resolver sequence into final storage.");
        writer.line("pub fn build(self) -> Resolvers<C, T> {");
        writer.indented(|writer| {
            writer.line("Resolvers {");
            writer.indented(|writer| {
                writer.line("groups: self.groups.into_boxed_slice(),");
                writer.line("subscription: self.subscription,");
                writer.line("marker: ::std::marker::PhantomData,");
            });
            writer.line("}");
        });
        writer.line("}");
    });
    writer.line("}");
    GeneratedItem::new("Resolvers", writer.finish()).exported()
}
