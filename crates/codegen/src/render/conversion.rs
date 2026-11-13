//! Generated input/output and runtime type-erasure conversion code.

mod expression;
mod required;

use chigiri_core::schema::{SemanticType, TypeKind};

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter, rust_string_literal};

use super::model::{RenderModel, enum_conversion_name, field_name, variant_name};
pub(super) use expression::{
    input_field_presence_expression, input_function, input_non_null_expression,
    input_presence_expression, output_expression, output_function,
};

pub(super) fn render_conversion_items(
    model: &RenderModel<'_>,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let required = required::names(model);
    model
        .user_types()
        .filter(|ty| required.contains(ty.name()))
        .map(|ty| render_named_conversion(model, ty))
        .collect()
}

fn render_named_conversion(
    model: &RenderModel<'_>,
    ty: &SemanticType,
) -> Result<GeneratedItem, CodegenError> {
    let mut writer = RustWriter::new();
    match ty.kind() {
        TypeKind::Scalar => render_scalar(&mut writer, model, ty),
        TypeKind::Enum => render_enum(&mut writer, model, ty),
        TypeKind::InputObject if is_one_of(ty) => render_one_of(&mut writer, model, ty)?,
        TypeKind::InputObject => render_input_object(&mut writer, model, ty)?,
        TypeKind::Object => render_object_output(&mut writer, model, ty),
        TypeKind::Interface | TypeKind::Union => render_composite(&mut writer, model, ty),
    }
    Ok(GeneratedItem::new(
        format!("internal/coercion/{}.rs", super::layout::owner(ty.name())),
        writer.finish(),
    ))
}

fn render_scalar(writer: &mut RustWriter, model: &RenderModel<'_>, ty: &SemanticType) {
    let associated = model.type_name(ty.name());
    let input = input_function(ty.name());
    let output = output_function(ty.name());
    writer.line(&format!(
        "pub(super) fn {input}<T: super::SchemaTypes>(value: ::chigiri::Value) -> ::chigiri::FieldResult<T::{associated}> {{"
    ));
    writer.indented(|writer| {
        writer.line(&format!(
            "<T::{associated} as ::chigiri::Scalar>::parse(value).map_err(::chigiri::__private::scalar_input_error)"
        ));
    });
    writer.line("}");
    writer.blank_line();
    writer.line(&format!(
        "pub(super) fn {output}<T: super::SchemaTypes>(value: T::{associated}) -> ::chigiri::FieldResult<::chigiri::__private::ResolvedValue> {{"
    ));
    writer.indented(|writer| {
        writer.line("::chigiri::Scalar::serialize(&value)");
        writer.indented(|writer| writer.line(".map(::chigiri::__private::ResolvedValue::Leaf)"));
    });
    writer.line("}");
}

fn render_enum(writer: &mut RustWriter, model: &RenderModel<'_>, ty: &SemanticType) {
    let rust = model.type_name(ty.name());
    let bound = model.enum_is_bound(ty.name());
    render_value_header(
        writer,
        &input_function(ty.name()),
        &if bound {
            format!("T::{rust}")
        } else {
            format!("super::{rust}")
        },
    );
    writer.indented(|writer| {
        writer.line("let ::chigiri::Value::String(value) = value else {");
        writer.indented(|writer| invariant_return(writer, "Enum value has the wrong runtime kind."));
        writer.line("};");
        writer.line("let wire = match value.as_str() {");
        writer.indented(|writer| {
            for value in ty.enum_values() {
                writer.line(&format!(
                    "{} => super::{rust}::{},",
                    rust_string_literal(&value.name.value),
                    variant_name(&value.name.value)
                ));
            }
            writer.line(
                "_ => return Err (:: chigiri::__private::invariant_error (\" enum value is undefined\")),",
            );
        });
        writer.line("};");
        if bound {
            writer.line(&format!(
                "T::{}(wire).map_err(::chigiri::__private::enum_binding_error)",
                enum_conversion_name(ty.name(), "from")
            ));
        } else {
            writer.line("Ok(wire)");
        }
    });
    writer.line("}");
    writer.blank_line();
    render_output_header(
        writer,
        &output_function(ty.name()),
        &if bound {
            format!("T::{rust}")
        } else {
            format!("super::{rust}")
        },
    );
    writer.indented(|writer| {
        if bound {
            writer.line(&format!(
                "let value = T::{}(&value).map_err(::chigiri::__private::enum_binding_error)?;",
                enum_conversion_name(ty.name(), "to")
            ));
        }
        writer.line("let value = match value {");
        writer.indented(|writer| {
            for value in ty.enum_values() {
                writer.line(&format!(
                    "super::{rust}::{} => {},",
                    variant_name(&value.name.value),
                    rust_string_literal(&value.name.value)
                ));
            }
        });
        writer.line("};");
        writer.line("::chigiri::__private::output_leaf(value)");
    });
    writer.line("}");
}

fn render_input_object(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    ty: &SemanticType,
) -> Result<(), CodegenError> {
    let rust = model.type_name(ty.name());
    let output = format!("super::{rust}{}", generic_parameter(model, ty));
    render_value_header(writer, &input_function(ty.name()), &output);
    writer.indented(|writer| {
        writer.line("let ::chigiri::Value::Object(mut object) = value else {");
        writer.indented(|writer| invariant_return(writer, "Input-object value has the wrong kind."));
        writer.line("};");
        writer.line(&format!(
            "let converted = super::{rust}{} {{",
            generic_constructor(model, ty)
        ));
        writer.indented(|writer| {
            for field in ty.input_fields() {
                let source = format!(
                    "object.remove({}).map_or(::chigiri::core::executable::CoercedInput::Undefined, |value| if value.is_null() {{ ::chigiri::core::executable::CoercedInput::Null }} else {{ ::chigiri::core::executable::CoercedInput::Value(value) }})",
                    rust_string_literal(&field.name.value)
                );
                let expression =
                    input_field_presence_expression(model, ty.name(), field, &source)?;
                writer.line(&format!("{}: {expression}?,", field_name(&field.name.value)));
            }
            Ok::<(), CodegenError>(())
        })?;
        writer.line("};");
        writer.line("if !object.is_empty() {");
        writer.indented(|writer| invariant_return(writer, "input object contains an unknown field"));
        writer.line("}");
        writer.line("Ok(converted)");
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(())
}

fn render_one_of(
    writer: &mut RustWriter,
    model: &RenderModel<'_>,
    ty: &SemanticType,
) -> Result<(), CodegenError> {
    let rust = model.type_name(ty.name());
    let output = format!("super::{rust}{}", generic_parameter(model, ty));
    render_value_header(writer, &input_function(ty.name()), &output);
    writer.indented(|writer| {
        writer.line("let ::chigiri::Value::Object(object) = value else {");
        writer
            .indented(|writer| invariant_return(writer, "OneOf input has the wrong runtime kind."));
        writer.line("};");
        writer.line("let mut fields = object.into_iter();");
        writer.line("let Some((name, value)) = fields.next() else {");
        writer.indented(|writer| invariant_return(writer, "OneOf has no field."));
        writer.line("};");
        writer.line("if fields.next().is_some() || value.is_null() {");
        writer.indented(|writer| {
            invariant_return(
                writer,
                "OneOf input must contain exactly one non-null field.",
            )
        });
        writer.line("}");
        writer.line("match name.as_str() {");
        writer.indented(|writer| {
            for field in ty.input_fields() {
                let mut converted = input_non_null_expression(model, &field.ty, "value")?;
                if model.input_field_is_boxed(ty.name(), &field.name.value) {
                    converted = format!("({converted}).map(::std::boxed::Box::new)");
                }
                writer.line(&format!(
                    "{} => {converted}.map(super::{rust}::{}),",
                    rust_string_literal(&field.name.value),
                    variant_name(&field.name.value)
                ));
            }
            writer.line(
                "_ => Err (:: chigiri::__private::invariant_error (\" OneOf field is undefined\")),",
            );
            Ok::<(), CodegenError>(())
        })?;
        writer.line("}");
        Ok::<(), CodegenError>(())
    })?;
    writer.line("}");
    Ok(())
}

fn render_object_output(writer: &mut RustWriter, model: &RenderModel<'_>, ty: &SemanticType) {
    let associated = model.type_name(ty.name());
    render_output_header(
        writer,
        &output_function(ty.name()),
        &format!("T::{associated}"),
    );
    writer.indented(|writer| {
        writer.line(&format!(
            "Ok(::chigiri::__private::ResolvedValue::object({}, value))",
            rust_string_literal(ty.name())
        ));
    });
    writer.line("}");
}

fn render_composite(writer: &mut RustWriter, model: &RenderModel<'_>, ty: &SemanticType) {
    let rust = model.type_name(ty.name());
    let input = format!(
        "super::{rust}{}",
        if model.composite_requires_schema_types(ty.name()) {
            "<T>"
        } else {
            ""
        }
    );
    render_output_header(writer, &output_function(ty.name()), &input);
    writer.indented(|writer| {
        writer.line("match value {");
        writer.indented(|writer| {
            for object in model.possible_objects(ty) {
                let variant = model.type_name(object.name());
                writer.line(&format!(
                    "super::{rust}::{variant}(value) => super::{}::<T>(value),",
                    output_function(object.name())
                ));
            }
        });
        writer.line("}");
    });
    writer.line("}");
}

fn render_value_header(writer: &mut RustWriter, function: &str, output: &str) {
    render_uniform_generic_allow(writer);
    writer.line(&format!(
        "pub(super) fn {function}<T: super::SchemaTypes>(value: ::chigiri::Value) -> ::chigiri::FieldResult<{output}> {{"
    ));
}

fn render_output_header(writer: &mut RustWriter, function: &str, input: &str) {
    render_uniform_generic_allow(writer);
    writer.line(&format!(
        "pub(super) fn {function}<T: super::SchemaTypes>(value: {input}) -> ::chigiri::FieldResult<::chigiri::__private::ResolvedValue> {{"
    ));
}

fn render_uniform_generic_allow(writer: &mut RustWriter) {
    writer.line("#[allow(");
    writer.indented(|writer| {
        writer.line("clippy::extra_unused_type_parameters,");
        writer.line(
            "reason = \"to keep conversion helper generic call forms consistent across schemas\"",
        );
    });
    writer.line(")]");
}

fn generic_parameter(model: &RenderModel<'_>, ty: &SemanticType) -> &'static str {
    if model.input_is_generic(ty.name()) {
        "<T>"
    } else {
        ""
    }
}

fn generic_constructor(model: &RenderModel<'_>, ty: &SemanticType) -> &'static str {
    if model.input_is_generic(ty.name()) {
        "::<T>"
    } else {
        ""
    }
}

fn is_one_of(ty: &SemanticType) -> bool {
    ty.directives()
        .iter()
        .any(|directive| directive.name.value == "oneOf")
}

fn invariant_return(writer: &mut RustWriter, message: &str) {
    writer.line(&format!(
        "return Err(::chigiri::__private::invariant_error({}));",
        rust_string_literal(message)
    ));
}
