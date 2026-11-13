//! Register every concrete non-root object as a storage model and resolve its optional identity.

use std::collections::BTreeMap;

use chigiri::core::schema::TypeKind;

use super::directives::{self, Role};
use crate::ir::{Model, ModelId, SchemaGraph, TypeId};
use crate::schema::SchemaError;

pub(super) fn collect(
    graph: &SchemaGraph,
) -> Result<(Vec<Model>, BTreeMap<TypeId, ModelId>), SchemaError> {
    let mut models = Vec::new();
    let mut types = BTreeMap::new();
    for (index, ty) in graph.types().iter().enumerate() {
        let id = TypeId(index);
        if ty.kind != TypeKind::Object
            || id == graph.query()
            || Some(id) == graph.mutation()
            || Some(id) == graph.subscription()
            || ty.name.starts_with("__")
        {
            continue;
        }
        if models.len() == crate::limits::MODELS {
            return Err(SchemaError::new("model count exceeded", ty.span));
        }
        let key = ty.fields.get("id").copied().filter(|field| {
            let node = &graph.fields()[field.0];
            let named = &graph.types()[node.ty.named.0];
            node.arguments.is_empty()
                && node.ty.list_depth() == 0
                && (named.kind == TypeKind::Enum
                    || matches!(named.name.as_ref(), "ID" | "String" | "Int" | "Boolean"))
                && matches!(
                    directives::role(&graph.field_source(*field).unwrap().directives),
                    Ok(Role::Plain)
                )
        });
        let auto_id = key.is_some_and(|key| {
            let node = &graph.fields()[key.0];
            !node.ty.nullable()
                && matches!(
                    graph.types()[node.ty.named.0].name.as_ref(),
                    "ID" | "String"
                )
        });
        if let Some(key) = key {
            let node = &graph.fields()[key.0];
            if matches!(
                graph.types()[node.ty.named.0].name.as_ref(),
                "ID" | "String"
            ) {
                crate::ir::constraints::validate_key(&node.constraints, node.span)?;
            }
        }
        types.insert(id, ModelId(models.len()));
        models.push(Model {
            ty: id,
            key,
            auto_id,
            slots: BTreeMap::new(),
            required_slots: Vec::new(),
            immutable_slots: Default::default(),
        });
    }
    Ok((models, types))
}
