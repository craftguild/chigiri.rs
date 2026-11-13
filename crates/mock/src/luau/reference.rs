//! Generate DTO and GraphQL output reference types while preserving nullable and presence semantics.

use chigiri::core::schema::TypeKind;

use super::text::Text;
use super::{ScriptError, names};
use crate::ir::{MockIr, TypeId, TypeRef, Wrapper};

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Output,
    Stored,
}

pub(super) fn reference(
    out: &mut Text,
    ir: &MockIr,
    ty: &TypeRef,
    mode: Mode,
) -> Result<(), ScriptError> {
    write_ref(out, ir, ty.named, &ty.wrappers, mode, true)
}

/// Render stored-slot nullability without redundant outer parentheses.
/// This preserves the canonical AST shape when the caller adds an omission union.
pub(super) fn stored_slot(out: &mut Text, ir: &MockIr, ty: &TypeRef) -> Result<(), ScriptError> {
    write_ref(out, ir, ty.named, &ty.wrappers, Mode::Stored, false)
}

/// Render one output alternative without nesting a nullable union in a union.
pub(super) fn output_alternative(
    out: &mut Text,
    ir: &MockIr,
    ty: &TypeRef,
) -> Result<(), ScriptError> {
    write_ref(out, ir, ty.named, &ty.wrappers, Mode::Output, false)
}

fn write_ref(
    out: &mut Text,
    ir: &MockIr,
    named: TypeId,
    wrappers: &[Wrapper],
    mode: Mode,
    grouped: bool,
) -> Result<(), ScriptError> {
    let nullable = wrappers.first() != Some(&Wrapper::NonNull);
    let wrappers = if nullable { wrappers } else { &wrappers[1..] };
    if nullable && grouped {
        out.add("(")?;
    }
    if wrappers.first() == Some(&Wrapper::List) {
        out.add("{")?;
        write_ref(out, ir, named, &wrappers[1..], mode, true)?;
        out.add("}")?;
    } else {
        name(out, ir, named, mode)?;
    }
    if nullable {
        out.add(" | ChigiriNull")?;
        if grouped {
            out.add(")")?;
        }
    }
    Ok(())
}

pub(super) fn name(out: &mut Text, ir: &MockIr, ty: TypeId, mode: Mode) -> Result<(), ScriptError> {
    if matches!(mode, Mode::Output) && Some(ty) == ir.graph().subscription() {
        return out.add(&names::ty(ir, ty, ""));
    }
    let node = &ir.graph().types()[ty.0];
    let prefix = match node.kind {
        TypeKind::Scalar | TypeKind::Enum => "Value",
        TypeKind::InputObject => "Input",
        TypeKind::Object => match (mode, ir.model_for(ty).is_some()) {
            (Mode::Output, true) => "Record",
            (Mode::Output, false) => "Object",
            (Mode::Stored, _) => "Stored",
        },
        _ => match mode {
            Mode::Output => "Abstract",
            Mode::Stored => "StoredAbstract",
        },
    };
    out.add(&names::ty(ir, ty, prefix))
}
