//! Describe state types without exposing initial or runtime state values.

use super::type_graph::{ApiTypeDoc, Primitive, TypeFieldDoc, TypeShapeDoc as T};
use super::{ReferenceDocument, ReferenceError, budget::Budget};
use crate::hooks::{StateContract, StateFields, StateType};

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    state: Option<&StateContract>,
) -> Result<(), ReferenceError> {
    if let Some(state) = state {
        let fields = fields(doc, budget, "ChigiriState", state.fields())?;
        push(
            doc,
            budget,
            "ChigiriState",
            T::Object {
                fields,
                closed: true,
            },
        )?;
        let T::Object { fields, .. } = &doc
            .api_types
            .last()
            .expect("state type was just appended")
            .shape
        else {
            unreachable!()
        };
        let mut patch = Vec::new();
        let mut reserved = 2;
        for field in fields {
            let field = TypeFieldDoc {
                name: field.name.clone(),
                type_id: field.type_id.clone(),
                required: false,
                nullable: field.nullable,
            };
            budget.reserve_array_item(&mut reserved, &field)?;
            patch.push(field);
        }
        push(
            doc,
            budget,
            "ChigiriStatePatch",
            T::Object {
                fields: patch,
                closed: true,
            },
        )?;
    } else {
        // Without evaluated configuration, represent state as a generic JSON map
        // rather than incorrectly claiming that it is an empty closed object.
        for id in ["ChigiriState", "ChigiriStatePatch"] {
            push(
                doc,
                budget,
                id,
                T::Map {
                    value_type_id: "json".into(),
                    string_keys: true,
                },
            )?;
        }
    }
    Ok(())
}

fn fields(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    id: &str,
    types: &StateFields,
) -> Result<Vec<TypeFieldDoc>, ReferenceError> {
    let mut fields = Vec::new();
    let mut reserved = 2;
    for (name, ty) in types {
        let child = format!("{id}.{name}");
        let type_id = value(doc, budget, &child, ty)?;
        let field = TypeFieldDoc {
            name: name.clone(),
            type_id,
            required: true,
            nullable: false,
        };
        budget.reserve_array_item(&mut reserved, &field)?;
        fields.push(field);
    }
    Ok(fields)
}
fn value(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    id: &str,
    ty: &StateType,
) -> Result<String, ReferenceError> {
    let shape = match ty {
        StateType::String => return Ok("json_string".into()),
        StateType::Boolean => return Ok("boolean".into()),
        StateType::Number => return Ok("json_number".into()),
        StateType::Integer => T::Primitive {
            representation: Primitive::SafeInteger,
            min_bytes: None,
            max_bytes: None,
        },
        StateType::Object { fields: types } => T::Object {
            fields: fields(doc, budget, id, types)?,
            closed: true,
        },
        StateType::List { of } => T::Array {
            item_type_id: value(doc, budget, &format!("{id}/item"), of)?,
            dense: true,
            maximum: Some(crate::value::MAX_ITEMS),
        },
        StateType::Nullable { of } => T::Union {
            variant_type_ids: vec![
                value(doc, budget, &format!("{id}/value"), of)?,
                "null_sentinel".into(),
            ],
        },
    };
    push(doc, budget, id, shape)?;
    Ok(id.into())
}

pub(super) fn push(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    id: &str,
    shape: T,
) -> Result<(), ReferenceError> {
    budget.push(
        &mut doc.api_types,
        ApiTypeDoc {
            id: id.into(),
            shape,
        },
    )
}
