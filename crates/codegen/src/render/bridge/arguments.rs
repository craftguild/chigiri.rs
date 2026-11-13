//! Renders shared coercion and typed argument construction.

use chigiri_core::language::ast::FieldDefinition;

use crate::error::CodegenError;
use crate::writer::RustWriter;

use super::super::conversion::input_presence_expression;
use super::super::model::{RenderModel, field_name, variant_name};
use super::super::resolvers::args_type;

pub(super) fn render_coercion(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    definition: &FieldDefinition,
) -> Result<(), CodegenError> {
    writer.line("let mut __chigiri_values = __chigiri_arguments.into_values();");
    for (index, argument) in definition.arguments.iter().enumerate() {
        let source = "::chigiri::__private::next_argument(&mut __chigiri_values)";
        let converted = input_presence_expression(model, argument, source)?;
        writer.line(&format!(
            "let __chigiri_argument_{index:04} = {converted}?;"
        ));
    }
    writer.line("::chigiri::__private::finish_arguments(&mut __chigiri_values)?;");
    Ok(())
}

pub(super) fn render_typed(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    object: &str,
    definition: &FieldDefinition,
) -> Result<(), CodegenError> {
    if definition.arguments.is_empty() {
        return Ok(());
    }
    let rust_type = args_type(model, object, definition)?;
    let base = format!(
        "{}{}Args",
        model.type_name(object),
        variant_name(&definition.name.value)
    );
    let constructor = if rust_type.ends_with("<T>") {
        format!("{base}::<T>")
    } else {
        base
    };
    writer.line(&format!("let __chigiri_args = super::{constructor} {{"));
    writer.indented(|writer| {
        for (index, argument) in definition.arguments.iter().enumerate() {
            let name = field_name(&argument.name.value);
            writer.line(&format!("{name}: __chigiri_argument_{index:04},"));
        }
    });
    writer.line("};");
    Ok(())
}
