//! Build input and output conversion expressions for GraphQL type references.

use chigiri_core::language::ast::{InputValueDefinition, TypeRef, TypeRefKind};

use crate::error::CodegenError;

use super::super::model::RenderModel;

pub(in crate::render) fn input_presence_expression(
    model: &RenderModel<'_>,
    definition: &InputValueDefinition,
    source: &str,
) -> Result<String, CodegenError> {
    input_presence_expression_inner(model, definition, source, false)
}

pub(in crate::render) fn input_field_presence_expression(
    model: &RenderModel<'_>,
    owner: &str,
    definition: &InputValueDefinition,
    source: &str,
) -> Result<String, CodegenError> {
    input_presence_expression_inner(
        model,
        definition,
        source,
        model.input_field_is_boxed(owner, &definition.name.value),
    )
}

fn input_presence_expression_inner(
    model: &RenderModel<'_>,
    definition: &InputValueDefinition,
    source: &str,
    boxed: bool,
) -> Result<String, CodegenError> {
    let mut converted = input_non_null_expression(model, &definition.ty, "value")?;
    if boxed {
        converted = format!("({converted}).map(::std::boxed::Box::new)");
    }
    let helper = if matches!(definition.ty.kind, TypeRefKind::NonNull(_)) {
        "required_input"
    } else if definition.default_value.is_some() {
        "defaulted_input"
    } else {
        "optional_input"
    };
    let converter = if boxed {
        None
    } else {
        direct_input_function(&definition.ty)
    }
    .map_or_else(|| format!("|value| {converted}"), str::to_owned);
    Ok(format!(
        "::chigiri::__private::{helper}({source}, {converter})"
    ))
}

pub(in crate::render) fn output_expression(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    source: &str,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::NonNull(inner) => output_non_null_expression(model, inner, source),
        _ => {
            let converted = output_non_null_expression(model, ty, "value")?;
            let converter = direct_output_function(ty)
                .map_or_else(|| format!("|value| {converted}"), str::to_owned);
            Ok(format!(
                "::chigiri::__private::output_nullable({source}, {converter})"
            ))
        }
    }
}

pub(in crate::render) fn input_function(name: &str) -> String {
    crate::naming::prefixed_field_method("__chigiri_input_", name)
}

pub(in crate::render) fn output_function(name: &str) -> String {
    crate::naming::prefixed_field_method("__chigiri_output_", name)
}

pub(in crate::render) fn input_non_null_expression(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    source: &str,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::NonNull(inner) => input_non_null_expression(model, inner, source),
        TypeRefKind::List(item) => {
            let converter = direct_nullable_input_function(item).map(str::to_owned);
            let item_expression = input_value_expression(model, item, "item")?;
            let converter = converter.unwrap_or_else(|| format!("|item| {item_expression}"));
            Ok(format!(
                "::chigiri::__private::input_list({source}, {converter})"
            ))
        }
        TypeRefKind::Named(name) => match name.value.as_str() {
            "Int" => Ok(format!("::chigiri::__private::input_int({source})")),
            "Float" => Ok(format!("::chigiri::__private::input_float({source})")),
            "String" => Ok(format!("::chigiri::__private::input_string({source})")),
            "Boolean" => Ok(format!("::chigiri::__private::input_boolean({source})")),
            "ID" => Ok(format!("::chigiri::__private::input_id({source})")),
            other => Ok(format!("super::{}::<T>({source})", input_function(other))),
        },
    }
}

fn input_value_expression(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    source: &str,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::NonNull(inner) => input_non_null_expression(model, inner, source),
        _ => {
            let converted = input_non_null_expression(model, ty, "value")?;
            let converter = direct_input_function(ty)
                .map_or_else(|| format!("|value| {converted}"), str::to_owned);
            Ok(format!(
                "::chigiri::__private::nullable_input({source}, {converter})"
            ))
        }
    }
}

fn output_non_null_expression(
    model: &RenderModel<'_>,
    ty: &TypeRef,
    source: &str,
) -> Result<String, CodegenError> {
    match &ty.kind {
        TypeRefKind::NonNull(inner) => output_non_null_expression(model, inner, source),
        TypeRefKind::List(item) => {
            // A nullable item needs its Option wrapper even for built-in leaves.
            // A direct scalar function accepts only the non-null Rust value.
            let converter = matches!(item.kind, TypeRefKind::NonNull(_))
                .then(|| direct_output_function(item))
                .flatten()
                .map(str::to_owned);
            let item_expression = output_expression(model, item, "item")?;
            let converter = converter.unwrap_or_else(|| format!("|item| {item_expression}"));
            Ok(format!(
                "::chigiri::__private::output_list({source}, {converter})"
            ))
        }
        TypeRefKind::Named(name) => match name.value.as_str() {
            "Float" => Ok(format!("::chigiri::__private::output_float({source})")),
            "Int" | "String" | "Boolean" | "ID" => {
                Ok(format!("::chigiri::__private::output_leaf({source})"))
            }
            other => Ok(format!("super::{}::<T>({source})", output_function(other))),
        },
    }
}

fn direct_input_function(ty: &TypeRef) -> Option<&'static str> {
    let TypeRefKind::Named(name) = unwrap_non_null(ty) else {
        return None;
    };
    match name.value.as_str() {
        "Int" => Some("::chigiri::__private::input_int"),
        "Float" => Some("::chigiri::__private::input_float"),
        "String" => Some("::chigiri::__private::input_string"),
        "Boolean" => Some("::chigiri::__private::input_boolean"),
        "ID" => Some("::chigiri::__private::input_id"),
        _ => None,
    }
}

fn direct_nullable_input_function(ty: &TypeRef) -> Option<&'static str> {
    if matches!(&ty.kind, TypeRefKind::NonNull(_)) {
        direct_input_function(ty)
    } else {
        None
    }
}

fn direct_output_function(ty: &TypeRef) -> Option<&'static str> {
    let TypeRefKind::Named(name) = unwrap_non_null(ty) else {
        return None;
    };
    match name.value.as_str() {
        "Float" => Some("::chigiri::__private::output_float"),
        "Int" | "String" | "Boolean" | "ID" => Some("::chigiri::__private::output_leaf"),
        _ => None,
    }
}

fn unwrap_non_null(ty: &TypeRef) -> &TypeRefKind {
    match &ty.kind {
        TypeRefKind::NonNull(inner) => unwrap_non_null(inner),
        kind => kind,
    }
}
