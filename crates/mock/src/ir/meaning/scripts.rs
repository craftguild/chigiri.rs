//! Determine complete script coverage and permissions for root, argument-bearing, and manual fields.

use chigiri::core::schema::TypeKind;

use super::{
    Build,
    directives::{self, Role},
};
use crate::ir::{FieldId, FieldPlan, ScriptId, ScriptPlan, ScriptRole};
use crate::schema::SchemaError;

pub(super) fn collect(build: &mut Build) -> Result<(), SchemaError> {
    for (index, field) in build.graph.fields().iter().enumerate() {
        let id = FieldId(index);
        if build.graph.types()[field.parent.0].kind != TypeKind::Object {
            continue;
        }
        let source = build.graph.field_source(id).unwrap();
        let role = directives::role(&source.directives)?;
        let root = build.graph.is_operation_root(field.parent);
        if !root && field.arguments.is_empty() && role != Role::Manual {
            if build.plans[index].is_none() {
                return Err(build.error(id, "unresolved concrete field role"));
            }
            continue;
        }
        if build.scripts.len() == crate::limits::SCRIPTS {
            return Err(build.error(id, "script coordinate count exceeded"));
        }
        let script = ScriptId(build.scripts.len());
        let root_as_nested = root
            && build.graph.fields().iter().any(|output| {
                output.ty.named == field.parent
                    || build.graph.types()[output.ty.named.0]
                        .possible_types
                        .contains(&field.parent)
            });
        let role = if field.parent == build.graph.query() {
            ScriptRole::QueryRoot { root_as_nested }
        } else if Some(field.parent) == build.graph.mutation() {
            ScriptRole::MutationRoot { root_as_nested }
        } else if Some(field.parent) == build.graph.subscription() {
            ScriptRole::SubscriptionRoot { root_as_nested }
        } else {
            ScriptRole::Object
        };
        build.scripts.push(ScriptPlan { field: id, role });
        build.plans[index] = Some(FieldPlan::Script(script));
    }
    Ok(())
}
