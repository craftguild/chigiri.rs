//! Renders generated resolver argument structs.

use chigiri_core::language::ast::FieldDefinition;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::super::model::{RenderModel, field_name, variant_name};

pub(in crate::render) fn args_type(
    model: &RenderModel<'_>,
    object: &str,
    field: &FieldDefinition,
) -> Result<String, CodegenError> {
    let name = format!(
        "{}{}Args",
        model.type_name(object),
        variant_name(&field.name.value)
    );
    let generic = field
        .arguments
        .iter()
        .map(|argument| model.input_type(argument))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|ty| uses_schema_types(ty));
    Ok(format!("{name}{}", if generic { "<T>" } else { "" }))
}

pub(super) fn render_args(
    model: &RenderModel<'_>,
    object: &str,
    field: &FieldDefinition,
) -> Result<GeneratedItem, CodegenError> {
    let name = format!(
        "{}{}Args",
        model.type_name(object),
        variant_name(&field.name.value)
    );
    let fields = field
        .arguments
        .iter()
        .map(|argument| {
            Ok((
                field_name(&argument.name.value),
                model.input_type(argument)?,
                argument
                    .description
                    .as_ref()
                    .map(|value| value.value.value.as_str()),
            ))
        })
        .collect::<Result<Vec<_>, CodegenError>>()?;
    let generic = fields.iter().any(|(_, ty, _)| uses_schema_types(ty));
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "/// Coerced arguments passed to the `{object}.{}` resolver.",
        field.name.value
    ));
    writer.line(&format!(
        "pub struct {name}{} {{",
        if generic {
            "<T: super::SchemaTypes>"
        } else {
            ""
        }
    ));
    writer.indented(|writer| {
        for (name, ty, description) in &fields {
            if let Some(description) = description {
                writer.doc_comment(description);
            }
            writer.line(&format!("pub {name}: {ty},"));
        }
    });
    writer.line("}");
    Ok(GeneratedItem::new(name, writer.finish()).exported())
}

fn uses_schema_types(ty: &str) -> bool {
    ty.contains("T::") || ty.contains("<T>")
}
