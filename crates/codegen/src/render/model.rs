//! Shared name and type mappings derived from the semantic schema for all renderers.

mod input_graph;
mod names;
mod type_mapping;

use ::std::collections::{BTreeMap, BTreeSet};

use chigiri_core::SourceMap;
use chigiri_core::language::ast::{InputValueDefinition, TypeRef, TypeRefKind};
use chigiri_core::schema::{SemanticSchema, SemanticType, TypeKind};

use crate::config::ResolvedBindings;
use crate::error::CodegenError;
use crate::naming::{IdentifierCase, IdentifierScope, raw_identifier, rust_identifier, snake_case};

use self::input_graph::analyze_input_graph;
use self::names::{
    diagnostic_error, resolver_method_names, source_name, validate_member_names,
    validate_public_names,
};
use self::type_mapping::{map_input_non_null, map_input_non_null_boxed, map_output};

pub(crate) struct RenderModel<'a> {
    pub(super) schema: &'a SemanticSchema,
    type_names: BTreeMap<String, String>,
    input_generics: BTreeSet<String>,
    boxed_input_edges: BTreeSet<(String, String)>,
    possible_objects: BTreeMap<String, Vec<&'a SemanticType>>,
    root_names: BTreeSet<String>,
    domain_objects: BTreeSet<String>,
    resolver_method_names: BTreeMap<String, String>,
    bound_enums: BTreeSet<String>,
}

impl<'a> RenderModel<'a> {
    #[cfg(test)]
    pub(super) fn build(schema: &'a SemanticSchema) -> Result<Self, CodegenError> {
        Self::build_inner(schema, None, &ResolvedBindings::default())
    }

    #[cfg(test)]
    pub(super) fn build_with_sources(
        schema: &'a SemanticSchema,
        sources: &SourceMap,
    ) -> Result<Self, CodegenError> {
        Self::build_inner(schema, Some(sources), &ResolvedBindings::default())
    }

    pub(crate) fn build_with_bindings(
        schema: &'a SemanticSchema,
        sources: &SourceMap,
        bindings: &ResolvedBindings,
    ) -> Result<Self, CodegenError> {
        Self::build_inner(schema, Some(sources), bindings)
    }

    fn build_inner(
        schema: &'a SemanticSchema,
        sources: Option<&SourceMap>,
        bindings: &ResolvedBindings,
    ) -> Result<Self, CodegenError> {
        let mut type_scope = IdentifierScope::default();
        let mut type_names = BTreeMap::new();
        for ty in user_types(schema) {
            let name = source_name(ty)?;
            let rust = type_scope
                .reserve(&name, IdentifierCase::UpperCamel)
                .map_err(|diagnostic| diagnostic_error(diagnostic, sources))?;
            type_names.insert(ty.name().to_owned(), rust);
        }
        let root_names = roots(schema);
        let domain_objects = domain_objects(schema, &root_names);
        validate_member_names(schema, &domain_objects, sources)?;
        validate_public_names(schema, &type_names, bindings.names(), sources)?;
        let resolver_method_names = resolver_method_names(schema, sources)?;
        let input_graph = analyze_input_graph(schema, bindings.names());
        let possible_objects = possible_object_index(schema);
        Ok(Self {
            schema,
            type_names,
            input_generics: input_graph.generics,
            boxed_input_edges: input_graph.boxed_edges,
            possible_objects,
            root_names,
            domain_objects,
            resolver_method_names,
            bound_enums: bindings.names().clone(),
        })
    }

    pub(crate) fn user_types(&self) -> impl Iterator<Item = &'a SemanticType> {
        user_types(self.schema)
    }

    pub(crate) fn type_name<'b>(&'b self, graphql: &'b str) -> &'b str {
        self.type_names
            .get(graphql)
            .map(String::as_str)
            .unwrap_or(graphql)
    }

    pub(crate) fn is_root(&self, graphql: &str) -> bool {
        self.root_names.contains(graphql)
    }

    pub(crate) fn is_subscription_root(&self, graphql: &str) -> bool {
        self.subscription_root() == Some(graphql)
    }

    pub(crate) fn subscription_root(&self) -> Option<&str> {
        self.schema
            .roots()
            .subscription()
            .map(|root| root.type_name.as_str())
    }

    pub(super) fn input_is_generic(&self, graphql: &str) -> bool {
        self.input_generics.contains(graphql)
    }

    pub(super) fn object_requires_domain(&self, graphql: &str) -> bool {
        self.domain_objects.contains(graphql)
    }

    pub(crate) fn resolver_method_name(&self, graphql: &str) -> &str {
        self.resolver_method_names[graphql].as_str()
    }

    pub(super) fn enum_is_bound(&self, graphql: &str) -> bool {
        self.bound_enums.contains(graphql)
    }

    pub(super) fn possible_objects(&self, ty: &SemanticType) -> &[&'a SemanticType] {
        self.possible_objects
            .get(ty.name())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn composite_requires_schema_types(&self, graphql: &str) -> bool {
        self.possible_objects
            .get(graphql)
            .is_some_and(|objects| !objects.is_empty())
    }

    pub(crate) fn output_type(&self, ty: &TypeRef) -> Result<String, CodegenError> {
        map_output(self, ty, 0)
    }

    pub(super) fn input_type(
        &self,
        definition: &InputValueDefinition,
    ) -> Result<String, CodegenError> {
        if let TypeRefKind::NonNull(inner) = &definition.ty.kind {
            return map_input_non_null(self, inner, 0);
        }
        let present = map_input_non_null(self, &definition.ty, 0)?;
        if definition.default_value.is_some() {
            Ok(format!("::std::option::Option<{present}>"))
        } else {
            Ok(format!("::chigiri::Input<{present}>"))
        }
    }

    pub(super) fn input_field_type(
        &self,
        owner: &str,
        definition: &InputValueDefinition,
    ) -> Result<String, CodegenError> {
        let boxed = self.input_field_is_boxed(owner, &definition.name.value);
        let map = |ty, depth| {
            if boxed {
                map_input_non_null_boxed(self, ty, depth)
            } else {
                map_input_non_null(self, ty, depth)
            }
        };
        if let TypeRefKind::NonNull(inner) = &definition.ty.kind {
            return map(inner, 0);
        }
        let present = map(&definition.ty, 0)?;
        if definition.default_value.is_some() {
            Ok(format!("::std::option::Option<{present}>"))
        } else {
            Ok(format!("::chigiri::Input<{present}>"))
        }
    }

    pub(super) fn input_field_is_boxed(&self, owner: &str, field: &str) -> bool {
        self.boxed_input_edges
            .contains(&(owner.to_owned(), field.to_owned()))
    }

    pub(super) fn one_of_input_type(
        &self,
        owner: &str,
        definition: &InputValueDefinition,
    ) -> Result<String, CodegenError> {
        if self.input_field_is_boxed(owner, &definition.name.value) {
            map_input_non_null_boxed(self, &definition.ty, 0)
        } else {
            map_input_non_null(self, &definition.ty, 0)
        }
    }
}

pub(crate) fn field_name(name: &str) -> String {
    rust_identifier(name, IdentifierCase::Snake)
}

pub(super) fn enum_conversion_name(name: &str, direction: &str) -> String {
    raw_identifier(format!("{}_{direction}_graphql", snake_case(name)))
}

pub(super) fn variant_name(name: &str) -> String {
    rust_identifier(name, IdentifierCase::UpperCamel)
}

fn user_types(schema: &SemanticSchema) -> impl Iterator<Item = &SemanticType> {
    schema.types().iter().filter(|ty| ty.name_span().is_some())
}

fn possible_object_index(schema: &SemanticSchema) -> BTreeMap<String, Vec<&SemanticType>> {
    let mut index = user_types(schema)
        .filter(|ty| matches!(ty.kind(), TypeKind::Interface | TypeKind::Union))
        .map(|ty| (ty.name().to_owned(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for object in user_types(schema).filter(|ty| ty.kind() == TypeKind::Object) {
        for interface in object.implemented_interfaces() {
            if let Some(objects) = index.get_mut(&interface.value) {
                objects.push(object);
            }
        }
    }
    for union in user_types(schema).filter(|ty| ty.kind() == TypeKind::Union) {
        let objects = union
            .union_members()
            .iter()
            .filter_map(|member| schema.type_by_name(&member.value))
            .collect();
        index.insert(union.name().to_owned(), objects);
    }
    index
}

fn roots(schema: &SemanticSchema) -> BTreeSet<String> {
    [
        schema.roots().query(),
        schema.roots().mutation(),
        schema.roots().subscription(),
    ]
    .into_iter()
    .flatten()
    .map(|root| root.type_name.clone())
    .collect()
}

fn domain_objects(schema: &SemanticSchema, roots: &BTreeSet<String>) -> BTreeSet<String> {
    let mut objects = user_types(schema)
        .filter(|ty| ty.kind() == TypeKind::Object && !roots.contains(ty.name()))
        .map(|ty| ty.name().to_owned())
        .collect::<BTreeSet<_>>();
    for root in roots {
        let Some(root_type) = schema.type_by_name(root) else {
            continue;
        };
        let is_referenced = user_types(schema)
            .flat_map(SemanticType::fields)
            .any(|field| named_ref(&field.ty) == root);
        let is_composite_member = !root_type.implemented_interfaces().is_empty()
            || user_types(schema)
                .filter(|ty| ty.kind() == TypeKind::Union)
                .any(|ty| ty.has_union_member(root));
        if is_referenced || is_composite_member {
            objects.insert(root.clone());
        }
    }
    objects
}

pub(super) fn named_ref(mut ty: &TypeRef) -> &str {
    loop {
        match &ty.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => ty = inner,
        }
    }
}
