//! Build a validated semantic schema once, resolving types and members to stable IDs.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::language::ast::{
    FieldDefinition, InputValueDefinition, TypeRef as AstRef, TypeRefKind,
};
use chigiri::core::schema::TypeKind;
use chigiri::core::{SchemaDigest, Source};
use chigiri::experimental::OwnedSchema;

use super::constraints::PatternRegistry;
use super::{
    Constraints, FieldId, FieldNode, InputId, InputNode, ScalarSpec, TypeId, TypeNode, TypeRef,
    Wrapper,
};
use crate::schema::{SchemaError, parse_snapshot, parse_sources};

/// Source snapshot, digest, and resolved ID graph kept in one immutable instance.
pub struct SchemaGraph {
    source: OwnedSchema,
    names: BTreeMap<Arc<str>, TypeId>,
    types: Vec<TypeNode>,
    fields: Vec<FieldNode>,
    inputs: Vec<InputNode>,
    query: TypeId,
    mutation: Option<TypeId>,
    subscription: Option<TypeId>,
    startup_work: crate::value::work::Work,
}

impl SchemaGraph {
    pub(crate) fn startup_work(&self) -> &crate::value::work::Work {
        &self.startup_work
    }

    /// Compose input SDL sources, validate them, and resolve all references.
    pub fn parse(sources: Vec<Source>) -> Result<Self, SchemaError> {
        Self::build(parse_sources(sources)?)
    }

    /// Verify a source snapshot and rebuild the same resolved graph.
    pub fn from_snapshot(
        sources: Vec<Source>,
        digest: SchemaDigest,
        version: &str,
    ) -> Result<Self, SchemaError> {
        Self::build(parse_snapshot(sources, digest, version)?)
    }

    /// Return the owned source snapshot and digest.
    pub fn source(&self) -> &OwnedSchema {
        &self.source
    }
    /// Return all named types in ID order.
    pub fn types(&self) -> &[TypeNode] {
        &self.types
    }
    /// Return all fields in ID order.
    pub fn fields(&self) -> &[FieldNode] {
        &self.fields
    }
    /// Return all input fields and arguments in ID order.
    pub fn inputs(&self) -> &[InputNode] {
        &self.inputs
    }
    /// Resolve a name to an existing type ID.
    pub fn type_id(&self, name: &str) -> Option<TypeId> {
        self.names.get(name).copied()
    }
    /// Return the query root type ID.
    pub const fn query(&self) -> TypeId {
        self.query
    }
    /// Return the mutation root type ID when present.
    pub const fn mutation(&self) -> Option<TypeId> {
        self.mutation
    }
    /// Return the subscription root type ID when present.
    pub const fn subscription(&self) -> Option<TypeId> {
        self.subscription
    }

    /// Whether this type is one of the schema's three operation roots.
    pub(crate) fn is_operation_root(&self, ty: TypeId) -> bool {
        ty == self.query || Some(ty) == self.mutation || Some(ty) == self.subscription
    }

    /// Borrow field directives from the original validated definition.
    pub fn field_source(&self, id: FieldId) -> Option<&FieldDefinition> {
        let field = self.fields.get(id.0)?;
        self.source.semantic().types()[field.parent.0].field(&field.name)
    }

    /// Resolve an original SDL type reference through this graph's name index.
    pub(crate) fn resolve_ref(&self, reference: &AstRef) -> Result<TypeRef, SchemaError> {
        resolve(&self.names, reference)
    }

    fn build(source: OwnedSchema) -> Result<Self, SchemaError> {
        let mut names = BTreeMap::new();
        let mut types = Vec::new();
        for (index, ty) in source.semantic().types().iter().enumerate() {
            let name: Arc<str> = Arc::from(ty.name());
            let enum_values: Vec<Arc<str>> = ty
                .enum_values()
                .iter()
                .map(|value| Arc::from(value.name.value.as_str()))
                .collect();
            names.insert(name.clone(), TypeId(index));
            types.push(TypeNode {
                name,
                kind: ty.kind(),
                span: ty.name_span(),
                scalar: ScalarSpec::read(ty)?,
                fields: BTreeMap::new(),
                inputs: BTreeMap::new(),
                required_inputs: Vec::new(),
                enum_index: enum_values.iter().cloned().collect(),
                enum_values,
                interfaces: Vec::new(),
                possible_types: Vec::new(),
                one_of: ty
                    .directives()
                    .iter()
                    .any(|directive| directive.name.value == "oneOf"),
            });
        }
        let query = source
            .semantic()
            .roots()
            .query()
            .and_then(|root| names.get(root.type_name.as_str()))
            .copied()
            .ok_or_else(|| SchemaError::new("query root required", None))?;
        let mutation = source
            .semantic()
            .roots()
            .mutation()
            .and_then(|root| names.get(root.type_name.as_str()))
            .copied();
        let subscription = source
            .semantic()
            .roots()
            .subscription()
            .and_then(|root| names.get(root.type_name.as_str()))
            .copied();
        let mut patterns = PatternRegistry::default();
        let mut fields = Vec::new();
        let mut inputs = Vec::new();
        for (index, ty) in source.semantic().types().iter().enumerate() {
            for field in ty.fields() {
                let id = FieldId(fields.len());
                let name: Arc<str> = Arc::from(field.name.value.as_str());
                let arguments: Vec<InputId> = field
                    .arguments
                    .iter()
                    .map(|input| add_input(&names, &types, &mut inputs, input, &mut patterns))
                    .collect::<Result<_, _>>()?;
                types[index].fields.insert(name.clone(), id);
                fields.push(FieldNode {
                    parent: TypeId(index),
                    name,
                    ty: resolve(&names, &field.ty)?,
                    argument_names: arguments
                        .iter()
                        .map(|id| (inputs[id.0].name.clone(), *id))
                        .collect(),
                    arguments,
                    span: field.name.span,
                    constraints: Constraints::field(
                        &types,
                        &resolve(&names, &field.ty)?,
                        &field.directives,
                        &mut patterns,
                    )?,
                });
            }
            for input in ty.input_fields() {
                let id = add_input(&names, &types, &mut inputs, input, &mut patterns)?;
                types[index].inputs.insert(inputs[id.0].name.clone(), id);
                if !inputs[id.0].ty.nullable() || inputs[id.0].default.is_some() {
                    types[index].required_inputs.push(id);
                }
            }
            types[index].interfaces = ty
                .implemented_interfaces()
                .iter()
                .map(|name| names[name.value.as_str()])
                .collect();
            types[index].possible_types = match ty.kind() {
                TypeKind::Object => vec![TypeId(index)],
                TypeKind::Union => ty
                    .union_members()
                    .iter()
                    .map(|name| names[name.value.as_str()])
                    .collect(),
                _ => Vec::new(),
            };
        }
        for index in 0..types.len() {
            if types[index].kind == TypeKind::Object {
                for interface in types[index].interfaces.clone() {
                    types[interface.0].possible_types.push(TypeId(index));
                }
            }
        }
        for ty in &mut types {
            ty.possible_types.sort();
            ty.possible_types.dedup();
        }
        Ok(Self {
            startup_work: patterns.into_work(),
            source,
            names,
            types,
            fields,
            inputs,
            query,
            mutation,
            subscription,
        })
    }
}

fn add_input(
    names: &BTreeMap<Arc<str>, TypeId>,
    types: &[TypeNode],
    inputs: &mut Vec<InputNode>,
    input: &InputValueDefinition,
    patterns: &mut PatternRegistry,
) -> Result<InputId, SchemaError> {
    let id = InputId(inputs.len());
    inputs.push(InputNode {
        name: Arc::from(input.name.value.as_str()),
        ty: resolve(names, &input.ty)?,
        default: input.default_value.clone(),
        span: input.name.span,
        constraints: Constraints::field(
            types,
            &resolve(names, &input.ty)?,
            &input.directives,
            patterns,
        )?,
    });
    Ok(id)
}

fn resolve(names: &BTreeMap<Arc<str>, TypeId>, reference: &AstRef) -> Result<TypeRef, SchemaError> {
    let mut wrappers = Vec::new();
    let mut current = reference;
    let named = loop {
        match &current.kind {
            TypeRefKind::Named(name) => {
                break names
                    .get(name.value.as_str())
                    .copied()
                    .ok_or_else(|| SchemaError::new("unresolved type", Some(name.span)))?;
            }
            TypeRefKind::List(inner) => {
                wrappers.push(Wrapper::List);
                current = inner;
            }
            TypeRefKind::NonNull(inner) => {
                wrappers.push(Wrapper::NonNull);
                current = inner;
            }
        }
    };
    Ok(TypeRef {
        named,
        wrappers: Arc::from(wrappers),
    })
}
