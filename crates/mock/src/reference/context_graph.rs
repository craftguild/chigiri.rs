//! Map context values and JSON constructors to stable, machine-readable types.

use super::type_graph::{
    ApiTypeDoc, CallingConvention, Primitive, TypeFieldDoc, TypeShapeDoc as T,
};
use super::{ReferenceDocument, ReferenceError, budget::Budget};
use crate::api::context as contract;

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    let mut fields: Vec<_> = [
        (contract::SCOPE, "context_scope"),
        (contract::NULL, "null_sentinel"),
        (contract::MODELS, "model_collection"),
        (crate::api::hooks::STATE, "ChigiriState"),
    ]
    .into_iter()
    .map(|(name, ty)| member(name, ty))
    .collect();
    let mut push = |id: &str, shape| {
        budget.push(
            &mut doc.api_types,
            ApiTypeDoc {
                id: id.into(),
                shape,
            },
        )
    };
    for constructor in contract::CONSTRUCTORS {
        let ty = constructor.shape.type_id();
        push(
            constructor.id,
            T::Function {
                calling_convention: CallingConvention::Dot,
                arity: constructor.arity,
                argument_type_ids: vec![ty.into(); constructor.arity],
                returns_type_id: ty.into(),
                returns_same_table: true,
                json_marker: constructor.shape,
            },
        )?;
        fields.push(member(constructor.name, constructor.id));
    }
    push(
        "context",
        T::Object {
            fields,
            closed: true,
        },
    )?;
    push(
        "context_scope",
        T::Enumeration {
            values: vec![contract::READ.into(), contract::MUTATION.into()],
        },
    )?;
    push("null_sentinel", T::NullSentinel { lua_nil: false })?;
    push(
        "model_collection",
        T::ModelCollection {
            exact_graphql_names: true,
            context_capability: true,
        },
    )?;
    push(
        "json_object",
        T::Map {
            value_type_id: "json".into(),
            string_keys: true,
        },
    )?;
    push(
        "json_array",
        T::Array {
            item_type_id: "json".into(),
            dense: true,
            maximum: Some(crate::value::MAX_ITEMS),
        },
    )?;
    push(
        "json",
        T::Union {
            variant_type_ids: [
                "null_sentinel",
                "json_string",
                "json_number",
                "boolean",
                "json_object",
                "json_array",
            ]
            .map(String::from)
            .to_vec(),
        },
    )?;
    push(
        "json_string",
        T::Primitive {
            representation: Primitive::String,
            min_bytes: None,
            max_bytes: Some(crate::value::MAX_STRING_BYTES),
        },
    )?;
    push(
        "json_number",
        T::Primitive {
            representation: Primitive::FiniteExactNumber,
            min_bytes: None,
            max_bytes: None,
        },
    )?;
    Ok(())
}

fn member(name: &str, ty: &str) -> TypeFieldDoc {
    // The sentinel is distinct from Lua nil and represents an explicit JSON null value.
    TypeFieldDoc {
        name: name.into(),
        type_id: ty.into(),
        required: true,
        nullable: false,
    }
}
