//! Splits object `__typename` routing into a fixed-fan-out function tree.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

// Internal structure that bounds per-function branching and register pressure. It does not
// control file partitioning.
const ROUTES_PER_FUNCTION: usize = 32;

pub(super) struct SchemaRoutes {
    pub(super) root: String,
    pub(super) items: Vec<GeneratedItem>,
}

pub(super) fn render_schema_routes(objects: &[&SemanticType]) -> SchemaRoutes {
    let mut items = Vec::new();
    let mut routes = objects
        .chunks(ROUTES_PER_FUNCTION)
        .enumerate()
        .map(|(index, objects)| {
            let name = format!("__chigiri_schema_route_00_{index:04}");
            items.push(render_leaf(&name, index, objects));
            name
        })
        .collect::<Vec<_>>();
    let mut level = 1usize;
    while routes.len() > 1 {
        let mut parents = Vec::new();
        for (index, children) in routes.chunks(ROUTES_PER_FUNCTION).enumerate() {
            let name = format!("__chigiri_schema_route_{level:02}_{index:04}");
            items.push(render_branch(&name, children));
            parents.push(name);
        }
        routes = parents;
        level += 1;
    }
    SchemaRoutes {
        root: routes
            .pop()
            .expect("validated schema must contain an object route"),
        items,
    }
}

fn render_leaf(name: &str, chunk: usize, objects: &[&SemanticType]) -> GeneratedItem {
    let mut writer = RustWriter::new();
    render_header(&mut writer, name);
    writer.line("{");
    writer.indented(|writer| {
        writer.line("match __chigiri_parent_type {");
        writer.indented(|writer| {
            for (offset, object) in objects.iter().enumerate() {
                let index = chunk * ROUTES_PER_FUNCTION + offset;
                writer.line(&format!(
                    "{} => ::chigiri::__private::DispatchAttempt::Matched(__chigiri_resolvers.groups[{index}].resolve(__chigiri_context, __chigiri_field_name, __chigiri_parent, __chigiri_arguments)),",
                    rust_string_literal(object.name()),
                ));
            }
            writer.line("_ => ::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments),");
        });
        writer.line("}");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_branch(name: &str, children: &[String]) -> GeneratedItem {
    let mut writer = RustWriter::new();
    render_header(&mut writer, name);
    writer.line("{");
    writer.indented(|writer| {
        for child in children {
            writer.line(&format!(
                "let __chigiri_arguments = match super::{child}(__chigiri_resolvers, __chigiri_context, __chigiri_parent_type, __chigiri_field_name, __chigiri_parent, __chigiri_arguments) {{"
            ));
            writer.indented(|writer| {
                writer.line("::chigiri::__private::DispatchAttempt::Matched(__chigiri_future) => return ::chigiri::__private::DispatchAttempt::Matched(__chigiri_future),");
                writer.line("::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments) => __chigiri_arguments,");
            });
            writer.line("};");
        }
        writer.line("::chigiri::__private::DispatchAttempt::Unmatched(__chigiri_arguments)");
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_header(writer: &mut RustWriter, name: &str) {
    writer.line(&format!("pub(super) fn {name}<'a, C, T>("));
    writer.indented(|writer| {
        writer.line("__chigiri_resolvers: &'a super::Resolvers<C, T>,");
        writer.line("__chigiri_context: &'a C,");
        writer.line("__chigiri_parent_type: &'a str,");
        writer.line("__chigiri_field_name: &'a str,");
        writer.line(
            "__chigiri_parent: ::std::option::Option<&'a ::chigiri::__private::ResolvedObject>,",
        );
        writer.line("__chigiri_arguments: ::chigiri::__private::ArgumentValues,");
    });
    writer.line(") -> ::chigiri::__private::DispatchAttempt<::chigiri::__private::BoxFuture<'a, ::chigiri::FieldResult<::chigiri::__private::ResolvedValue>>>");
    writer.line("where");
    writer.indented(|writer| {
        writer.line("C: ::std::marker::Send + ::std::marker::Sync + 'static,");
        writer.line("T: super::SchemaTypes,");
    });
}
