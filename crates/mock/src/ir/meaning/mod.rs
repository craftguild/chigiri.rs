//! Resolve model and field responsibilities into one storage and execution IR.

mod directives;
mod fields;
mod keys;
mod models;
mod relation_declarations;
mod relation_matching;
mod relations;
mod scripts;
mod storage_declarations;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_scripts;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chigiri::core::{SchemaDigest, Source};

use super::{
    FieldId, FieldPlan, KeySpec, Model, ModelId, Relation, SchemaGraph, ScriptPlan, Slot, TypeId,
};
use crate::schema::SchemaError;

/// Resolved Mock declarations layered over the semantic type graph.
pub struct MockIr {
    scope: Arc<()>,
    public_graph: SchemaGraph,
    graph: SchemaGraph,
    public_source_count: usize,
    storage_source_count: usize,
    private_types: BTreeSet<TypeId>,
    private_fields: BTreeSet<FieldId>,
    models: Vec<Model>,
    model_types: BTreeMap<TypeId, ModelId>,
    slots: Vec<Slot>,
    keys: Vec<KeySpec>,
    relations: Vec<Relation>,
    fields: Vec<Option<FieldPlan>>,
    scripts: Vec<ScriptPlan>,
    required_values: Vec<Vec<FieldId>>,
}

impl MockIr {
    pub(crate) fn scope(&self) -> &Arc<()> {
        &self.scope
    }
    /// Validate user SDL and Mock declarations.
    pub fn parse(sources: Vec<Source>) -> Result<Self, SchemaError> {
        Self::parse_with_storage(sources, Vec::new())
    }

    /// Public GraphQL sources and private storage sources are validated independently,
    /// then compiled into one storage contract.
    pub fn parse_with_storage(
        public: Vec<Source>,
        storage: Vec<Source>,
    ) -> Result<Self, SchemaError> {
        let public_count = public.len();
        let storage_count = storage.len();
        storage_declarations::validate(&storage, public_count)?;
        let public_graph = SchemaGraph::parse(public.clone())?;
        let mut combined = public;
        combined.extend(storage);
        let graph = SchemaGraph::parse(combined)?;
        Self::build(graph, public_graph, public_count, storage_count)
    }

    /// Apply the same declaration validation to embedded source snapshots.
    pub fn from_snapshot(
        sources: Vec<Source>,
        digest: SchemaDigest,
        version: &str,
    ) -> Result<Self, SchemaError> {
        let public_count = sources.len();
        let public_graph = SchemaGraph::from_snapshot(sources.clone(), digest, version)?;
        let graph = SchemaGraph::from_snapshot(sources, digest, version)?;
        Self::build(graph, public_graph, public_count, 0)
    }

    /// Restore the separate public and storage roles from a portable bundle.
    pub fn from_snapshots(
        mut public: Vec<Source>,
        public_digest: SchemaDigest,
        storage: Vec<Source>,
        storage_digest: SchemaDigest,
        version: &str,
    ) -> Result<Self, SchemaError> {
        let public_count = public.len().saturating_sub(1);
        let storage_count = storage.len();
        storage_declarations::validate(&storage, public_count)?;
        let public_graph = SchemaGraph::from_snapshot(public.clone(), public_digest, version)?;
        let vocabulary = public
            .pop()
            .ok_or_else(|| SchemaError::new("missing vocabulary source", None))?;
        let mut combined = public;
        combined.extend(storage);
        combined.push(vocabulary);
        let graph = SchemaGraph::from_snapshot(combined, storage_digest, version)?;
        Self::build(graph, public_graph, public_count, storage_count)
    }

    /// Return the validated storage graph.
    pub fn graph(&self) -> &SchemaGraph {
        &self.graph
    }
    /// The schema exposed to GraphQL validation and introspection.
    pub fn public_graph(&self) -> &SchemaGraph {
        &self.public_graph
    }
    /// Whether the named type originates in storage-only SDL.
    pub fn is_private_type(&self, ty: TypeId) -> bool {
        self.private_types.contains(&ty)
    }
    /// Whether the field originates in storage-only SDL.
    pub fn is_private_field(&self, field: FieldId) -> bool {
        self.private_fields.contains(&field)
    }
    /// Public-role source texts in their declared order, excluding the vocabulary.
    pub fn public_sources(&self) -> &[Source] {
        &self.graph.source().sources()[..self.public_source_count]
    }
    /// Storage-role source texts in their declared order.
    pub fn storage_sources(&self) -> &[Source] {
        &self.graph.source().sources()
            [self.public_source_count..self.public_source_count + self.storage_source_count]
    }
    /// Return all models in declaration order.
    pub fn models(&self) -> &[Model] {
        &self.models
    }
    /// Return all storage slots.
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }
    /// Return reference keys composed from stored slots.
    pub fn keys(&self) -> &[KeySpec] {
        &self.keys
    }
    /// Return owner-side relations.
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }
    /// Return concrete field coordinates that require resolver scripts.
    pub fn scripts(&self) -> &[ScriptPlan] {
        &self.scripts
    }
    /// How to run a concrete field.
    pub fn field_plan(&self, field: FieldId) -> Option<&FieldPlan> {
        self.fields.get(field.0)?.as_ref()
    }
    /// Return the model ID when this type is stored.
    pub fn model_for(&self, ty: TypeId) -> Option<ModelId> {
        self.model_types.get(&ty).copied()
    }

    /// Iterate relation fields on this model, including owner and inverse sides.
    pub fn relation_fields(&self, model: ModelId) -> impl Iterator<Item = FieldId> + '_ {
        self.models
            .get(model.0)
            .into_iter()
            .flat_map(|model| self.graph.types()[model.ty.0].fields.values().copied())
            .filter(|field| {
                matches!(
                    self.field_plan(*field),
                    Some(FieldPlan::Owner(_) | FieldPlan::Inverse { .. })
                )
            })
    }

    /// Resolve a relation field name and its target model.
    pub(crate) fn relation_target(&self, model: ModelId, name: &str) -> Option<(FieldId, ModelId)> {
        let model = self.models.get(model.0)?;
        let field = *self.graph.types()[model.ty.0].fields.get(name)?;
        let target = match self.field_plan(field)? {
            FieldPlan::Owner(id) => self.relations[id.0].target,
            FieldPlan::Inverse { relation } => self.relations[relation.0].owner,
            _ => return None,
        };
        Some((field, target))
    }

    /// Return non-null stored shadow fields required in a DTO.
    pub fn required_value_fields(&self, ty: TypeId) -> &[FieldId] {
        self.required_values.get(ty.0).map_or(&[], Vec::as_slice)
    }

    fn build(
        graph: SchemaGraph,
        public_graph: SchemaGraph,
        public_count: usize,
        storage_count: usize,
    ) -> Result<Self, SchemaError> {
        let storage_end = public_count + storage_count;
        let private_types = graph
            .types()
            .iter()
            .enumerate()
            .filter(|(_, ty)| {
                ty.span.is_some_and(|span| {
                    (public_count..storage_end).contains(&span.source_id.index())
                })
            })
            .map(|(id, _)| TypeId(id))
            .collect();
        let private_fields = graph
            .fields()
            .iter()
            .enumerate()
            .filter(|(id, _)| {
                graph.field_source(FieldId(*id)).is_some_and(|field| {
                    (public_count..storage_end).contains(&field.span.source_id.index())
                })
            })
            .map(|(id, _)| FieldId(id))
            .collect();
        validate_storage(&graph, &private_types, &private_fields)?;
        let (models, model_types) = models::collect(&graph)?;
        let mut build = Build {
            plans: vec![None; graph.fields().len()],
            graph,
            models,
            model_types,
            slots: Vec::new(),
            keys: Vec::new(),
            relations: Vec::new(),
            scripts: Vec::new(),
        };
        fields::classify(&mut build)?;
        relations::resolve(&mut build)?;
        scripts::collect(&mut build)?;
        let mut required_values = vec![Vec::new(); build.graph.types().len()];
        for (index, field) in build.graph.fields().iter().enumerate() {
            if !field.ty.nullable()
                && matches!(
                    build.plans[index],
                    Some(FieldPlan::Value | FieldPlan::Stored(_))
                )
            {
                required_values[field.parent.0].push(FieldId(index));
            }
        }
        let ir = Self {
            scope: Arc::new(()),
            public_graph,
            graph: build.graph,
            public_source_count: public_count,
            storage_source_count: storage_count,
            private_types,
            private_fields,
            models: build.models,
            model_types: build.model_types,
            slots: build.slots,
            keys: build.keys,
            relations: build.relations,
            fields: build.plans,
            scripts: build.scripts,
            required_values,
        };
        crate::value::validate_defaults(&ir)?;
        Ok(ir)
    }
}

fn validate_storage(
    graph: &SchemaGraph,
    private_types: &BTreeSet<TypeId>,
    private_fields: &BTreeSet<FieldId>,
) -> Result<(), SchemaError> {
    use chigiri::core::schema::TypeKind;
    for ty in private_types {
        let node = &graph.types()[ty.0];
        if !matches!(
            node.kind,
            TypeKind::Object | TypeKind::Enum | TypeKind::Scalar
        ) {
            return Err(SchemaError::new("unsupported private type", node.span));
        }
    }
    for field in private_fields {
        let node = &graph.fields()[field.0];
        if graph.is_operation_root(node.parent)
            || !node.arguments.is_empty()
            || matches!(
                directives::role(graph.field_source(*field).unwrap().directives.as_slice()),
                Ok(directives::Role::Manual) | Err(_)
            )
        {
            return Err(SchemaError::new(
                "invalid private storage field",
                Some(node.span),
            ));
        }
    }
    Ok(())
}

struct Build {
    graph: SchemaGraph,
    models: Vec<Model>,
    model_types: BTreeMap<TypeId, ModelId>,
    slots: Vec<Slot>,
    keys: Vec<KeySpec>,
    relations: Vec<Relation>,
    plans: Vec<Option<FieldPlan>>,
    scripts: Vec<ScriptPlan>,
}

impl Build {
    fn error(&self, field: FieldId, code: &'static str) -> SchemaError {
        SchemaError::new(code, Some(self.graph.fields()[field.0].span))
    }
}
