//! Split the custom-scalar registry into fixed-fan-out function trees.

use chigiri_core::schema::SemanticType;

use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

use super::scalar_validator_name;

// Internal structure that fixes the branching and register width of a function. It is not used for file partitioning conditions.
const REGISTRATIONS_PER_FUNCTION: usize = 32;

pub(super) struct ScalarRegistration {
    pub(super) root: Option<String>,
    pub(super) items: Vec<GeneratedItem>,
}

pub(super) fn render_registration(scalars: &[&SemanticType]) -> ScalarRegistration {
    let mut items = Vec::new();
    let mut functions = scalars
        .chunks(REGISTRATIONS_PER_FUNCTION)
        .enumerate()
        .map(|(index, scalars)| {
            let name = format!("__chigiri_register_scalars_00_{index:04}");
            items.push(render_leaf(&name, index, scalars));
            name
        })
        .collect::<Vec<_>>();
    let mut level = 1usize;
    while functions.len() > 1 {
        let mut parents = Vec::new();
        for (index, children) in functions.chunks(REGISTRATIONS_PER_FUNCTION).enumerate() {
            let name = format!("__chigiri_register_scalars_{level:02}_{index:04}");
            items.push(render_branch(&name, children));
            parents.push(name);
        }
        functions = parents;
        level += 1;
    }
    ScalarRegistration {
        root: functions.pop(),
        items,
    }
}

fn render_leaf(name: &str, chunk: usize, scalars: &[&SemanticType]) -> GeneratedItem {
    let mut writer = header(name);
    writer.indented(|writer| {
        for (offset, scalar) in scalars.iter().enumerate() {
            let index = chunk * REGISTRATIONS_PER_FUNCTION + offset;
            writer.line(&format!(
                "__chigiri_registry.register({}, super::{}::<T>);",
                rust_string_literal(scalar.name()),
                scalar_validator_name(index)
            ));
        }
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn render_branch(name: &str, children: &[String]) -> GeneratedItem {
    let mut writer = header(name);
    writer.indented(|writer| {
        for child in children {
            writer.line(&format!("super::{child}::<T>(__chigiri_registry);"));
        }
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

fn header(name: &str) -> RustWriter {
    let mut writer = RustWriter::new();
    writer.line(&format!("pub(super) fn {name}<T: super::SchemaTypes>("));
    writer.indented(|writer| {
        writer.line("__chigiri_registry: &mut ::chigiri::core::executable::CustomScalarRegistry,");
    });
    writer.line(") {");
    writer
}
