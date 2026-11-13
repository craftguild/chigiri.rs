//! Render directive applications and arguments as independent items and lists.

use chigiri_core::language::ast::Directive;

use crate::error::CodegenError;
use crate::literal::graphql_value;
use crate::writer::{GeneratedItem, RustWriter};

use super::lists::{descriptor_list_expression, render_descriptor_list};
use super::values::{literal, render_const_value_items};

pub(super) fn render(
    stem: &str,
    directives: &[Directive],
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = Vec::new();
    for (directive_index, directive) in directives.iter().enumerate() {
        let directive_stem = format!("{stem}_{directive_index:04}");
        for (argument_index, argument) in directive.arguments.iter().enumerate() {
            let name = format!("{directive_stem}_ARGUMENT_{argument_index:04}");
            let value = graphql_value(&argument.value).map_err(CodegenError::from_message)?;
            let (value_items, value_descriptor) =
                render_const_value_items(&format!("{name}_VALUE"), &argument.value.value)?;
            items.extend(value_items);
            let mut writer = RustWriter::new();
            writer.line(&format!(
                "pub(super) const {name}: ::chigiri::DirectiveArgumentDescriptor = ::chigiri::DirectiveArgumentDescriptor {{"
            ));
            writer.indented(|writer| {
                writer.line(&format!("name: {},", literal(&argument.name.value)));
                writer.line(&format!("value: {},", literal(&value)));
                writer.line(&format!("value_descriptor: {value_descriptor},"));
                Ok::<(), CodegenError>(())
            })?;
            writer.line("};");
            items.push(GeneratedItem::new(name, writer.finish()));
        }
        items.extend(render_descriptor_list(
            &format!("{directive_stem}_ARGUMENT"),
            directive.arguments.len(),
            "::chigiri::DirectiveArgumentDescriptor",
        ));
        let mut writer = RustWriter::new();
        writer.line(&format!(
            "pub(super) const {directive_stem}: ::chigiri::AppliedDirectiveDescriptor = ::chigiri::AppliedDirectiveDescriptor {{"
        ));
        writer.indented(|writer| {
            writer.line(&format!("name: {},", literal(&directive.name.value)));
            writer.line(&format!(
                "arguments: {},",
                descriptor_list_expression(
                    &format!("{directive_stem}_ARGUMENT"),
                    directive.arguments.len()
                )
            ));
        });
        writer.line("};");
        items.push(GeneratedItem::new(directive_stem, writer.finish()));
    }
    items.extend(render_descriptor_list(
        stem,
        directives.len(),
        "::chigiri::AppliedDirectiveDescriptor",
    ));
    Ok(items)
}

pub(super) fn expression(stem: &str, directives: &[Directive]) -> String {
    descriptor_list_expression(stem, directives.len())
}
