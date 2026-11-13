//! Generate stable aliases for scalars, enums, inputs, DTOs, and resolver arguments.

use chigiri::core::schema::TypeKind;

use super::names;
use super::reference::{self, Mode};
use super::text::Text;
use super::{MAX_WRAPPER, ScriptError, models};
use crate::ir::{FieldPlan, MockIr, Representation, ScriptId, TypeId, TypeNode};

pub(super) fn generate(
    ir: &MockIr,
    state: &crate::hooks::StateContract,
) -> Result<String, ScriptError> {
    let mut out = Text::new(MAX_WRAPPER);
    runtime_capabilities(&mut out)?;
    out.add("-- Type aliases preserve SDL names. Table keys preserve normal SDL field names. ChigiriNull distinguishes explicit null from omission.\n")?;
    out.add("type ChigiriJson = ChigiriNull | ChigiriJsonValue\ntype ChigiriJsonValue = string | number | boolean | ChigiriJsonObject | ChigiriJsonArray\ntype ChigiriJsonObject = {[string]: ChigiriJson}\ntype ChigiriJsonArray = {ChigiriJson}\n")?;
    out.add(crate::project::CONFIG_DECLARATIONS)?;
    for (id, node) in ir.graph().types().iter().enumerate() {
        let ty = TypeId(id);
        match node.kind {
            TypeKind::Scalar => scalar(&mut out, ir, ty, node)?,
            TypeKind::Enum => {
                out.format(format_args!("type {} = ", names::ty(ir, ty, "Value")))?;
                for (index, value) in node.enum_values.iter().enumerate() {
                    if index > 0 {
                        out.add(" | ")?;
                    }
                    out.quoted(value)?;
                }
                out.add("\n")?;
            }
            TypeKind::InputObject => input(&mut out, ir, ty, node)?,
            TypeKind::Object => {
                if ir.model_for(ty).is_none()
                    && !ir.is_private_type(ty)
                    && Some(ty) != ir.graph().subscription()
                {
                    dto(&mut out, ir, ty, node, Mode::Output, false)?;
                }
                dto(&mut out, ir, ty, node, Mode::Stored, false)?;
                dto(&mut out, ir, ty, node, Mode::Stored, true)?;
            }
            TypeKind::Interface | TypeKind::Union => abstract_type(&mut out, ir, ty, node)?,
        }
    }
    models::generate(&mut out, ir)?;
    super::subscription::declarations::shared(&mut out, ir)?;
    super::subscription::declarations::setup_context(&mut out, ir)?;
    super::hook_types::generate(&mut out, state)?;
    super::consumer_types::generate(&mut out, ir)?;
    for (id, script) in ir.scripts().iter().enumerate() {
        out.format(format_args!(
            "type {} = {{\n",
            names::resolver(ir, ScriptId(id), "Args")
        ))?;
        for input in &ir.graph().fields()[script.field.0].arguments {
            let input = &ir.graph().inputs()[input.0];
            out.key(&input.name)?;
            reference::reference(&mut out, ir, &input.ty, Mode::Output)?;
            if input.ty.nullable() && input.default.is_none() {
                out.add("?")?;
            }
            out.add(",\n")?;
        }
        out.add("}\n")?;
        if script.role.is_subscription_root() {
            super::subscription::declarations::coordinate(&mut out, ir, ScriptId(id))?;
        } else {
            super::module_contract::declarations(&mut out, ir, ScriptId(id))?;
        }
    }
    Ok(out.finish())
}

pub(super) fn capability_declarations() -> Result<String, ScriptError> {
    let mut out = Text::new(MAX_WRAPPER);
    runtime_capabilities(&mut out)?;
    Ok(out.finish())
}

fn runtime_capabilities(out: &mut Text) -> Result<(), ScriptError> {
    out.format(format_args!(
        "-- Executable capability contract: {}\n",
        crate::api::runtime::VERSION
    ))?;
    out.add("type ChigiriOsDateInput = {year: number, month: number, day: number, hour: number?, min: number?, sec: number?, isdst: boolean?}\n")?;
    out.add("type ChigiriOsDateResult = {year: number, month: number, day: number, hour: number, min: number, sec: number, wday: number, yday: number, isdst: boolean}\n")?;
    out.add("type TimeleapOptions = {world: boolean?}\n")?;
    for library in crate::api::runtime::LIBRARIES {
        out.format(format_args!("type {} = {{\n", library.type_name))?;
        for function in library.functions {
            out.key(function.name)?;
            out.format(format_args!("{},\n", function.function_type()))?;
        }
        out.add("}\n")?;
    }
    out.add("type ChigiriGlobalLibrary = {\n")?;
    for function in crate::api::runtime::GLOBALS {
        out.key(function.name)?;
        out.format(format_args!("{},\n", function.function_type()))?;
    }
    for library in crate::api::runtime::LIBRARIES {
        out.key(library.name)?;
        out.format(format_args!("{},\n", library.type_name))?;
    }
    out.add("}\n")?;
    out.add("type ChigiriNoTimeleapGlobalLibrary = {\n")?;
    for function in crate::api::runtime::GLOBALS {
        out.key(function.name)?;
        out.format(format_args!("{},\n", function.function_type()))?;
    }
    for library in crate::api::runtime::LIBRARIES {
        if library.name != "timeleap" {
            out.key(library.name)?;
            out.format(format_args!("{},\n", library.type_name))?;
        }
    }
    out.add("}\n")
}

fn scalar(out: &mut Text, ir: &MockIr, ty: TypeId, node: &TypeNode) -> Result<(), ScriptError> {
    let value = if let Some(spec) = &node.scalar {
        match spec.representation {
            Representation::String => "string",
            Representation::Boolean => "boolean",
            Representation::Integer | Representation::Number => "number",
            Representation::Object => "ChigiriJsonObject",
            Representation::Array => "ChigiriJsonArray",
            Representation::Json => "ChigiriJsonValue",
        }
    } else {
        match node.name.as_ref() {
            "String" | "ID" => "string",
            "Boolean" => "boolean",
            "Int" | "Float" => "number",
            _ => {
                return Err(ScriptError::new(
                    "contract",
                    "scalar representation missing",
                ));
            }
        }
    };
    out.format(format_args!(
        "type {} = {value}\n",
        names::ty(ir, ty, "Value")
    ))
}

fn input(out: &mut Text, ir: &MockIr, ty: TypeId, node: &TypeNode) -> Result<(), ScriptError> {
    out.format(format_args!("type {} = ", names::ty(ir, ty, "Input")))?;
    if node.one_of {
        for (index, active) in node.inputs.values().enumerate() {
            if index > 0 {
                out.add(" | ")?;
            }
            out.add("{\n")?;
            for input in node.inputs.values() {
                let item = &ir.graph().inputs()[input.0];
                out.key(&item.name)?;
                if input == active {
                    let mut nonnull = item.ty.clone();
                    let mut wrappers = vec![crate::ir::Wrapper::NonNull];
                    wrappers.extend_from_slice(&item.ty.wrappers);
                    nonnull.wrappers = wrappers.into();
                    reference::reference(out, ir, &nonnull, Mode::Output)?;
                } else {
                    out.add("nil")?;
                }
                out.add(",\n")?;
            }
            out.add("}")?;
        }
        out.add("\n")?;
        return Ok(());
    }
    out.add("{\n")?;
    for input in node.inputs.values() {
        let input = &ir.graph().inputs()[input.0];
        out.key(&input.name)?;
        reference::reference(out, ir, &input.ty, Mode::Output)?;
        if input.ty.nullable() && input.default.is_none() {
            out.add("?")?;
        }
        out.add(",\n")?;
    }
    out.add("}\n")
}

fn dto(
    out: &mut Text,
    ir: &MockIr,
    ty: TypeId,
    node: &TypeNode,
    mode: Mode,
    concrete: bool,
) -> Result<(), ScriptError> {
    let prefix = match mode {
        Mode::Output => "Object",
        Mode::Stored if concrete => "ConcreteStored",
        Mode::Stored => "Stored",
    };
    out.format(format_args!("type {} = {{\n", names::ty(ir, ty, prefix)))?;
    typename(out, &node.name, !concrete)?;
    for field in node.fields.values() {
        if matches!(mode, Mode::Output) && ir.is_private_field(*field) {
            continue;
        }
        if !matches!(
            ir.field_plan(*field),
            Some(FieldPlan::Value | FieldPlan::Stored(_))
        ) {
            continue;
        }
        let field = &ir.graph().fields()[field.0];
        out.key(&field.name)?;
        reference::reference(out, ir, &field.ty, mode)?;
        if field.ty.nullable() {
            out.add("?")?;
        }
        out.add(",\n")?;
    }
    out.add("}\n")
}

fn abstract_type(
    out: &mut Text,
    ir: &MockIr,
    ty: TypeId,
    node: &TypeNode,
) -> Result<(), ScriptError> {
    for (prefix, stored) in [("Abstract", false), ("StoredAbstract", true)] {
        out.format(format_args!("type {} = ", names::ty(ir, ty, prefix)))?;
        let mut members = node
            .possible_types
            .iter()
            .filter(|member| stored || !ir.is_private_type(**member))
            .peekable();
        if members.peek().is_none() {
            out.add("never")?;
        }
        for (index, member) in members.enumerate() {
            if index > 0 {
                out.add(" | ")?;
            }
            if stored {
                out.add(&names::ty(ir, *member, "ConcreteStored"))?;
            } else if ir.model_for(*member).is_some() {
                out.add(&names::ty(ir, *member, "ConcreteRecord"))?;
            } else if Some(*member) == ir.graph().subscription() {
                out.format(format_args!("({} & {{\n", names::ty(ir, *member, "")))?;
                typename(out, &ir.graph().types()[member.0].name, false)?;
                out.add("})")?;
            } else {
                out.format(format_args!("({} & {{\n", names::ty(ir, *member, "Object")))?;
                typename(out, &ir.graph().types()[member.0].name, false)?;
                out.add("})")?;
            }
        }
        out.add("\n")?;
    }
    Ok(())
}

pub(super) fn typename(out: &mut Text, name: &str, optional: bool) -> Result<(), ScriptError> {
    out.key(crate::api::model_values::TYPENAME)?;
    out.quoted(name)?;
    if optional {
        out.add("?")?;
    }
    out.add(",\n")
}
