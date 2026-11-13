//! Build hook snapshot and context types from the shared host contract.

use super::api_types::ContextDoc;
use super::type_graph::{CallingConvention, Primitive, TypeFieldDoc, TypeShapeDoc as T};
use super::{ReferenceDocument, ReferenceError, budget::Budget, state_graph::push};
use crate::api::hook_values::{HookValue as V, LOG_FUNCTION, REJECT_FUNCTION};
use crate::api::{Effect, hooks as api};
use crate::hooks::HookSlot;

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    for (id, shape) in [
        (
            "hook_text",
            T::Primitive {
                representation: Primitive::String,
                min_bytes: None,
                max_bytes: Some(crate::limits::HOOK_OBSERVATION_TEXT_BYTES),
            },
        ),
        (
            "hook_number",
            T::Primitive {
                representation: Primitive::FiniteExactNumber,
                min_bytes: None,
                max_bytes: None,
            },
        ),
        (
            "hook_read_scope",
            T::Enumeration {
                values: vec![crate::api::context::READ.into()],
            },
        ),
        (
            "hook_json_object",
            T::Map {
                value_type_id: "hook_json".into(),
                string_keys: true,
            },
        ),
        (
            "hook_json_array",
            T::Array {
                item_type_id: "hook_json".into(),
                dense: true,
                maximum: Some(crate::value::MAX_ITEMS),
            },
        ),
        (
            "hook_json",
            T::Union {
                variant_type_ids: [
                    "null_sentinel",
                    "hook_text",
                    "hook_number",
                    "boolean",
                    "hook_json_object",
                    "hook_json_array",
                ]
                .map(String::from)
                .to_vec(),
            },
        ),
        ("hook_nil", T::Nil),
        ("hook_never", T::Never),
        ("hook_void", T::Void),
    ] {
        push(doc, budget, id, shape)?;
    }
    for snapshot in api::SNAPSHOTS {
        let mut fields = Vec::new();
        for (name, ty) in snapshot.fields {
            let type_id = value(
                doc,
                budget,
                &format!("{}.{name}", snapshot.alias),
                ty,
                if snapshot.alias == "ChigiriHookRejectReason" {
                    "json_string"
                } else {
                    "hook_text"
                },
            )?;
            fields.push(member(name, type_id, !matches!(ty, V::Optional(_))));
        }
        push(
            doc,
            budget,
            snapshot.alias,
            T::Object {
                fields,
                closed: true,
            },
        )?;
    }
    for function in [&LOG_FUNCTION, &REJECT_FUNCTION] {
        let mut arguments = Vec::new();
        for (name, ty) in function.arguments {
            let type_id = value(
                doc,
                budget,
                &format!("{}.{name}", function.alias),
                ty,
                "json_string",
            )?;
            arguments.push(member(name, type_id, !matches!(ty, V::Optional(_))));
        }
        push(
            doc,
            budget,
            function.alias,
            T::HostFunction {
                calling_convention: CallingConvention::Dot,
                minimum_arity: function.minimum_arity(),
                maximum_arity: function.arguments.len(),
                arguments,
                returns_type_id: if function.diverges {
                    "hook_never"
                } else {
                    "hook_void"
                }
                .into(),
            },
        )?;
    }
    for slot in HookSlot::ALL {
        let id = api::alias(slot, "Context");
        let mut fields = vec![
            member(crate::api::context::SCOPE, "hook_read_scope".into(), true),
            member(crate::api::context::NULL, "null_sentinel".into(), true),
            member(crate::api::context::MODELS, "model_collection".into(), true),
            member(api::STATE, "ChigiriState".into(), true),
            member(LOG_FUNCTION.name, LOG_FUNCTION.alias.into(), true),
        ];
        for constructor in crate::api::context::CONSTRUCTORS {
            fields.push(member(constructor.name, constructor.id.into(), true));
        }
        if slot.allows_reject() {
            fields.push(member(
                REJECT_FUNCTION.name,
                REJECT_FUNCTION.alias.into(),
                true,
            ));
        }
        for (name, ty) in api::phase_fields(slot) {
            fields.push(member(name, ty.into(), true));
        }
        push(
            doc,
            budget,
            &id,
            T::Object {
                fields,
                closed: true,
            },
        )?;
        let result = api::alias(slot, "Result");
        let mut variants = vec!["hook_nil".into()];
        if slot.allows_patch() {
            let envelope = api::alias(slot, "PatchReturn");
            push(
                doc,
                budget,
                &envelope,
                T::Object {
                    fields: vec![member(api::PATCH, "ChigiriStatePatch".into(), true)],
                    closed: true,
                },
            )?;
            variants.push(envelope);
        }
        push(
            doc,
            budget,
            &result,
            T::Union {
                variant_type_ids: variants,
            },
        )?;
        let run = api::alias(slot, "Run");
        push(
            doc,
            budget,
            &run,
            T::HostFunction {
                calling_convention: CallingConvention::Dot,
                minimum_arity: 1,
                maximum_arity: 1,
                arguments: vec![member("ctx", id.clone(), true)],
                returns_type_id: result,
            },
        )?;
        push(
            doc,
            budget,
            &api::alias(slot, "Hook"),
            T::Object {
                fields: vec![member(api::RUN, run, true)],
                closed: true,
            },
        )?;
        budget.push(
            &mut doc.contexts,
            ContextDoc {
                id: format!("hook/{}", slot.name()),
                type_id: id,
                scope_value: crate::api::context::READ.into(),
                effect: Effect::Read,
                method_ids: crate::api::METHODS
                    .iter()
                    .filter(|method| method.effect == Effect::Read)
                    .map(|method| method.id.into())
                    .collect(),
                rule_ids: vec!["hook.state".into(), "hook.phase".into(), "hook.log".into()],
            },
        )?;
    }
    Ok(())
}
fn member(name: &str, type_id: String, required: bool) -> TypeFieldDoc {
    TypeFieldDoc {
        name: name.into(),
        type_id,
        required,
        nullable: false,
    }
}
fn value(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    id: &str,
    ty: &V,
    text: &str,
) -> Result<String, ReferenceError> {
    let shape = match ty {
        V::Text => return Ok(text.into()),
        V::Number => return Ok("hook_number".into()),
        V::Json => {
            return Ok(if text == "hook_text" {
                "hook_json"
            } else {
                "json"
            }
            .into());
        }
        V::JsonObject => {
            return Ok(if text == "hook_text" {
                "hook_json_object"
            } else {
                "json_object"
            }
            .into());
        }
        V::Null => return Ok("null_sentinel".into()),
        V::Named(name) => return Ok((*name).into()),
        V::Optional(inner) => return value(doc, budget, id, inner, text),
        V::Array(item) => T::Array {
            item_type_id: value(doc, budget, &format!("{id}/item"), item, text)?,
            dense: true,
            maximum: Some(crate::value::MAX_ITEMS),
        },
        V::Union(values) => {
            let mut variants = Vec::new();
            for (index, ty) in values.iter().enumerate() {
                variants.push(value(doc, budget, &format!("{id}/{index}"), ty, text)?);
            }
            T::Union {
                variant_type_ids: variants,
            }
        }
        V::Enumeration(values) => T::Enumeration {
            values: values.iter().map(|value| (*value).into()).collect(),
        },
    };
    push(doc, budget, id, shape)?;
    Ok(id.into())
}
