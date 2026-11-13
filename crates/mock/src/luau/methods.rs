//! Render method signatures from shared patterns and reuse read-method receivers across capabilities.

use super::text::Text;
use super::{ScriptError, names};
use crate::api::{COUNT_OPTIONS, FIND_OPTIONS, METHODS, OptionType};
use crate::ir::{MockIr, ModelId, TypeId};

pub(super) fn generate(
    out: &mut Text,
    ir: &MockIr,
    id: usize,
    ty: TypeId,
) -> Result<(), ScriptError> {
    let owners = ir.relation_fields(ModelId(id)).collect::<Vec<_>>();
    if !owners.is_empty() {
        out.format(format_args!("type {} = ", names::ty(ir, ty, "Relation")))?;
        for (index, relation) in owners.iter().enumerate() {
            if index > 0 {
                out.add(" | ")?;
            }
            out.quoted(&ir.graph().fields()[relation.0].name)?;
        }
        out.add("\n")?;
    }
    for (prefix, options) in [
        ("FindOptions", FIND_OPTIONS),
        ("CountOptions", COUNT_OPTIONS),
    ] {
        out.format(format_args!("type {} = {{\n", names::ty(ir, ty, prefix)))?;
        for option in options {
            out.key(option.name)?;
            out.add("(")?;
            match option.ty {
                OptionType::Where => out.add(&names::ty(ir, ty, "Where"))?,
                OptionType::OrderBy => {
                    out.format(format_args!("{{{}}}", names::ty(ir, ty, "OrderKey")))?
                }
                OptionType::Integer(bounds) => {
                    if bounds
                        .default
                        .is_some_and(|default| default > bounds.maximum)
                    {
                        return Err(ScriptError::new("contract", "invalid option bounds"));
                    }
                    out.add("number")?;
                }
            }
            if option.nullable {
                out.add(" | ChigiriNull")?;
            }
            out.add(")?,\n")?;
        }
        out.add("}\n")?;
    }
    for spec in METHODS {
        if !spec.eligibility.allows(ir, ModelId(id)) {
            continue;
        }
        super::method_signatures::render(out, ir, ModelId(id), spec)?;
    }
    // Reuse one alias so read operations remain callable on the read/write capability union.
    for (prefix, writable) in [("ReadModel", false), ("WriteModel", true)] {
        out.format(format_args!("type {} = {{\n", names::ty(ir, ty, prefix)))?;
        for spec in METHODS {
            if spec.effect.allowed(writable) && spec.eligibility.allows(ir, ModelId(id)) {
                out.key(spec.name)?;
                out.format(format_args!("{},\n", names::method(ir, ty, spec.name)))?;
            }
        }
        out.add("}\n")?;
    }
    Ok(())
}
