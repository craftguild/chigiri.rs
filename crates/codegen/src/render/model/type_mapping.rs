//! Converts GraphQL type references to generated public Rust types.

use chigiri_core::language::ast::{TypeRef, TypeRefKind};
use chigiri_core::schema::TypeKind;

use crate::error::CodegenError;

use super::RenderModel;

pub(super) fn map_output(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    depth: usize,
) -> Result<String, CodegenError> {
    if depth > 256 {
        return Err(CodegenError::new(
            "SF4007",
            "The output nesting is too deep.",
        ));
    }
    match &ty.kind {
        TypeRefKind::NonNull(inner) => map_output_non_null(model, inner, depth + 1),
        _ => Ok(format!(
            "::std::option::Option<{}>",
            map_output_non_null(model, ty, depth + 1)?
        )),
    }
}

fn map_output_non_null(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    depth: usize,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::Named(name) => named_type(model, &name.value, true),
        TypeRefKind::List(item) => Ok(format!(
            "::std::vec::Vec<{}>",
            map_output(model, item, depth + 1)?
        )),
        TypeRefKind::NonNull(inner) => map_output_non_null(model, inner, depth + 1),
    }
}

fn map_input(model: &RenderModel<'_>, ty: &TypeRef, depth: usize) -> Result<String, CodegenError> {
    if depth > 256 {
        return Err(CodegenError::new(
            "SF4007",
            "Input type nesting exceeds the supported depth.",
        ));
    }
    match &ty.kind {
        TypeRefKind::NonNull(inner) => map_input_non_null(model, inner, depth + 1),
        _ => Ok(format!(
            "::std::option::Option<{}>",
            map_input_non_null(model, ty, depth + 1)?
        )),
    }
}

pub(super) fn map_input_non_null(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    depth: usize,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::Named(name) => named_type(model, &name.value, false),
        TypeRefKind::List(item) => Ok(format!(
            "::std::vec::Vec<{}>",
            map_input(model, item, depth + 1)?
        )),
        TypeRefKind::NonNull(inner) => map_input_non_null(model, inner, depth + 1),
    }
}

pub(super) fn map_input_non_null_boxed(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    depth: usize,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::Named(_) => Ok(format!(
            "::std::boxed::Box<{}>",
            map_input_non_null(model, ty, depth)?
        )),
        TypeRefKind::NonNull(inner) => map_input_non_null_boxed(model, inner, depth + 1),
        TypeRefKind::List(_) => map_input_non_null(model, ty, depth),
    }
}

fn named_type(model: &RenderModel<'_>, name: &str, output: bool) -> Result<String, CodegenError> {
    let mapped = match name {
        "Int" => "i32".to_owned(),
        "Float" => "f64".to_owned(),
        "String" => "::std::string::String".to_owned(),
        "Boolean" => "bool".to_owned(),
        "ID" => "::chigiri::Id".to_owned(),
        _ => {
            let ty = model.schema.type_by_name(name).ok_or_else(|| {
                CodegenError::new("SF4007", format!("Cannot render unknown type `{name}`."))
            })?;
            match ty.kind() {
                TypeKind::Scalar | TypeKind::Object if output => {
                    format!("T::{}", model.type_name(name))
                }
                TypeKind::Scalar => format!("T::{}", model.type_name(name)),
                TypeKind::Object => {
                    return Err(CodegenError::new(
                        "SF4007",
                        format!("The object `{name}` cannot be input type."),
                    ));
                }
                TypeKind::Enum if model.enum_is_bound(name) => {
                    format!("T::{}", model.type_name(name))
                }
                TypeKind::Enum | TypeKind::InputObject => {
                    let generic = model.input_is_generic(name);
                    format!(
                        "super::{}{}",
                        model.type_name(name),
                        if generic { "<T>" } else { "" }
                    )
                }
                TypeKind::Interface | TypeKind::Union if output => {
                    let generic = model.composite_requires_schema_types(name);
                    format!(
                        "super::{}{}",
                        model.type_name(name),
                        if generic { "<T>" } else { "" }
                    )
                }
                TypeKind::Interface | TypeKind::Union => {
                    return Err(CodegenError::new(
                        "SF4007",
                        format!("The composite `{name}` is not an input type."),
                    ));
                }
            }
        }
    };
    Ok(mapped)
}
