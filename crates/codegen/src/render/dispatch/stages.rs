//! Renders source-ordered resolver typestate transitions.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter};

use super::super::model::RenderModel;
use super::super::resolvers::resolver_trait;

pub(super) fn render_stage(
    model: &RenderModel<'_>,
    index: usize,
    object: &SemanticType,
) -> GeneratedItem {
    let mut writer = RustWriter::new();
    render_stage_method(&mut writer, model, index, object);
    GeneratedItem::new(format!("ResolverStage{index:04}"), writer.finish())
}

fn render_stage_method(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    index: usize,
    object: &SemanticType,
) {
    let next = index + 1;
    let method = model.resolver_method_name(object.name());
    let resolver = resolver_trait(model, object.name());
    let adapter = format!("{}Adapter", model.type_name(object.name()));
    writer.line(&format!(
        "impl<C, T, E> super::ResolverStage<C, T, E, {index}>"
    ));
    super::write_stage_bounds(writer);
    writer.line("{");
    writer.indented(|writer| {
        writer.line(&format!(
            "/// Add the `{}` object resolver in source order.",
            object.name()
        ));
        writer.line(&format!(
            "pub fn {method}<R>(mut self, resolver: R) -> super::ResolverStage<C, T, E, {next}>"
        ));
        writer.line("where");
        writer.indented(|writer| writer.line(&format!("R: super::{resolver}<C, T, E>,")));
        writer.line("{");
        writer.indented(|writer| {
            if model.is_subscription_root(object.name()) {
                writer.line(&format!(
                    "let __chigiri_adapter = ::std::sync::Arc::new(super::{adapter}::<C, T, R, E>::new(resolver, ::std::sync::Arc::clone(&self.mapper)));"
                ));
                writer.line("self.groups.push(::std::sync::Arc::clone(&__chigiri_adapter) as ::std::sync::Arc<dyn ::chigiri::__private::ResolverGroupDispatch<C>>);");
                writer.line("self.subscription = ::std::option::Option::Some(__chigiri_adapter);");
            } else {
                writer.line(&format!(
                    "self.groups.push(::std::sync::Arc::new(super::{adapter}::<C, T, R, E>::new(resolver, ::std::sync::Arc::clone(&self.mapper))));"
                ));
            }
            writer.line("super::ResolverStage {");
            writer.indented(|writer| {
                writer.line("groups: self.groups,");
                writer.line("subscription: self.subscription,");
                writer.line("mapper: self.mapper,");
                writer.line("marker: ::std::marker::PhantomData,");
            });
            writer.line("}");
        });
        writer.line("}");
    });
    writer.line("}");
}
