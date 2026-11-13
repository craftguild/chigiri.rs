//! Overload shared API methods by relation name while binding target options to the correct relation type.

use super::{ScriptError, names, text::Text};
use crate::api::{Effect, MethodSpec, TypeExpr};
use crate::ir::{FieldId, MockIr, ModelId, TypeId};

pub(super) fn render(
    out: &mut Text,
    ir: &MockIr,
    model: ModelId,
    spec: &MethodSpec,
) -> Result<(), ScriptError> {
    let ty = ir.models()[model.0].ty;
    out.format(format_args!("type {} = ", names::method(ir, ty, spec.name)))?;
    if spec
        .arguments
        .iter()
        .any(|arg| arg.ty == TypeExpr::TargetOptions)
    {
        for (index, field) in ir.relation_fields(model).enumerate() {
            if index != 0 {
                out.add(" & ")?;
            }
            function(out, ir, model, spec, Some(field))?;
        }
    } else {
        function(out, ir, model, spec, None)?;
    }
    out.add("\n")
}

fn function(
    out: &mut Text,
    ir: &MockIr,
    model: ModelId,
    spec: &MethodSpec,
    relation: Option<FieldId>,
) -> Result<(), ScriptError> {
    let ty = ir.models()[model.0].ty;
    let receiver = if spec.effect == Effect::Read {
        "ReadModel"
    } else {
        "WriteModel"
    };
    out.format(format_args!("(({}", names::ty(ir, ty, receiver)))?;
    for argument in spec.arguments {
        out.add(", ")?;
        match (argument.ty, relation) {
            (TypeExpr::Relation, Some(field)) => out.quoted(&ir.graph().fields()[field.0].name)?,
            (TypeExpr::TargetOptions, Some(field)) => {
                let (_, target) = ir
                    .relation_target(model, &ir.graph().fields()[field.0].name)
                    .ok_or_else(|| ScriptError::new("contract", "relation target required"))?;
                expression(out, ir, TypeExpr::FindOptions, ir.models()[target.0].ty)?;
            }
            (TypeExpr::TargetOptions, None) => {
                return Err(ScriptError::new(
                    "contract",
                    "target options require relation overload",
                ));
            }
            (ty_expr, _) => expression(out, ir, ty_expr, ty)?,
        }
    }
    out.add(") -> ")?;
    expression(out, ir, spec.returns, ty)?;
    out.add(")")
}

fn expression(out: &mut Text, ir: &MockIr, expr: TypeExpr, ty: TypeId) -> Result<(), ScriptError> {
    match expr {
        TypeExpr::FindOptions => out.add(&names::ty(ir, ty, "FindOptions")),
        TypeExpr::CountOptions => out.add(&names::ty(ir, ty, "CountOptions")),
        TypeExpr::Relation => out.add(&names::ty(ir, ty, "Relation")),
        TypeExpr::CreateInput => out.add(&names::ty(ir, ty, "CreateInput")),
        TypeExpr::UpdateInput => out.add(&names::ty(ir, ty, "UpdateInput")),
        TypeExpr::Record => out.add(&names::ty(ir, ty, "ConcreteRecord")),
        TypeExpr::Records => {
            out.format(format_args!("{{{}}}", names::ty(ir, ty, "ConcreteRecord")))
        }
        TypeExpr::Number => out.add("number"),
        TypeExpr::TargetOptions => Err(ScriptError::new("contract", "unbound target options")),
    }
}
