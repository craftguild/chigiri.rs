//! Convert schema type-reference forms into a common introspection view.

use crate::language::ast::{TypeRef, TypeRefKind};
use crate::schema::introspection::model::{TypeRefDescriptor, TypeWrapper};
use crate::schema::{SemanticSchema, SemanticTypeRef, SemanticTypeRefKind};

use super::model::{NamedType, TypeView};

pub(super) fn from_ast<'a>(reference: &'a TypeRef, schema: &'a SemanticSchema) -> TypeView<'a> {
    match &reference.kind {
        TypeRefKind::Named(name) => named(&name.value, schema),
        TypeRefKind::List(item) => TypeView::List(Box::new(from_ast(item, schema))),
        TypeRefKind::NonNull(inner) => TypeView::NonNull(Box::new(from_ast(inner, schema))),
    }
}

pub(super) fn from_semantic<'a>(
    reference: &'a SemanticTypeRef,
    schema: &'a SemanticSchema,
) -> TypeView<'a> {
    match &reference.kind {
        SemanticTypeRefKind::Named(name) => named(name, schema),
        SemanticTypeRefKind::List(item) => TypeView::List(Box::new(from_semantic(item, schema))),
        SemanticTypeRefKind::NonNull(inner) => {
            TypeView::NonNull(Box::new(from_semantic(inner, schema)))
        }
    }
}

pub(super) fn from_descriptor<'a>(
    reference: &TypeRefDescriptor,
    schema: &'a SemanticSchema,
) -> TypeView<'a> {
    let mut view = named(reference.named_type, schema);
    for wrapper in reference.wrappers.iter().rev() {
        view = match wrapper {
            TypeWrapper::List => TypeView::List(Box::new(view)),
            TypeWrapper::NonNull => TypeView::NonNull(Box::new(view)),
        };
    }
    view
}

pub(super) fn named<'a>(name: &str, schema: &'a SemanticSchema) -> TypeView<'a> {
    if let Some(ty) = schema.type_by_name(name) {
        TypeView::Named(NamedType::Semantic(ty))
    } else if let Some(ty) = crate::schema::introspection::type_by_name(name) {
        TypeView::Named(NamedType::Introspection(ty))
    } else {
        unreachable!("validated type reference is missing")
    }
}
