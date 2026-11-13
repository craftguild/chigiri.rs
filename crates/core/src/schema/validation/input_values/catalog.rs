//! Builds one reusable lookup for each named input type.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{TypeRef, TypeRefKind};

use super::{Catalog, ExpectedType, InputFieldShape, InputObjectShape, NamedShape, ScalarShape};
use crate::schema::{SemanticSchema, TypeKind};

impl Catalog {
    pub(super) fn build(schema: &SemanticSchema) -> Self {
        let mut named = BTreeMap::new();
        for named_type in &schema.types {
            let shape = match named_type.kind {
                TypeKind::Scalar => NamedShape::Scalar(scalar_shape(&named_type.name)),
                TypeKind::Enum => NamedShape::Enum(
                    named_type
                        .enum_values
                        .iter()
                        .map(|value| value.name.value.clone())
                        .collect::<BTreeSet<_>>(),
                ),
                TypeKind::InputObject => NamedShape::InputObject(input_object_shape(named_type)),
                TypeKind::Object | TypeKind::Interface | TypeKind::Union => continue,
            };
            named.insert(named_type.name.clone(), shape);
        }
        Self { named }
    }

    pub(super) fn get(&self, name: &str) -> Option<&NamedShape> {
        self.named.get(name)
    }
}

fn scalar_shape(name: &str) -> ScalarShape {
    match name {
        "Int" => ScalarShape::Int,
        "Float" => ScalarShape::Float,
        "String" => ScalarShape::String,
        "Boolean" => ScalarShape::Boolean,
        "ID" => ScalarShape::Id,
        _ => ScalarShape::Custom,
    }
}

fn input_object_shape(named_type: &crate::schema::SemanticType) -> InputObjectShape {
    let mut fields = BTreeMap::new();
    let mut required = Vec::new();
    for field in &named_type.input_fields {
        let is_required =
            matches!(field.ty.kind, TypeRefKind::NonNull(_)) && field.default_value.is_none();
        if is_required {
            required.push(field.name.value.clone());
        }
        fields.insert(
            field.name.value.clone(),
            InputFieldShape {
                ty: ExpectedType::from_source(&field.ty),
            },
        );
    }
    InputObjectShape {
        fields,
        required,
        one_of: named_type
            .directives
            .iter()
            .any(|directive| directive.name.value == "oneOf"),
    }
}

impl ExpectedType {
    pub(super) fn from_source(reference: &TypeRef) -> Self {
        match &reference.kind {
            TypeRefKind::Named(name) => Self::Named(name.value.as_str().into()),
            TypeRefKind::List(item) => Self::List(Box::new(Self::from_source(item))),
            TypeRefKind::NonNull(inner) => Self::NonNull(Box::new(Self::from_source(inner))),
        }
    }
}
