//! Restore definitions and extensions in document/source order.

use std::collections::BTreeMap;

use crate::language::ast::Document;
use crate::{SourceId, Span};

use super::{SchemaOrigin, SemanticSchema, SemanticType};

pub(super) fn restore(schema: &mut SemanticSchema, documents: &[Document]) {
    let document_order =
        documents
            .iter()
            .enumerate()
            .fold(BTreeMap::new(), |mut order, (index, document)| {
                order.entry(document.span.source_id).or_insert(index);
                order
            });
    for ty in &mut schema.types {
        restore_type(ty, &document_order);
    }
    schema
        .schema_directives
        .sort_by_key(|directive| span_key(directive.span, &document_order));
    schema
        .schema_origins
        .sort_by_key(|origin| origin_key(*origin, &document_order));
}

fn restore_type(ty: &mut SemanticType, document_order: &BTreeMap<SourceId, usize>) {
    reorder_directives(ty, document_order);
    ty.origins
        .sort_by_key(|origin| origin_key(*origin, document_order));
    ty.implements_interfaces
        .sort_by_key(|name| span_key(name.span, document_order));
    ty.union_members
        .sort_by_key(|name| span_key(name.span, document_order));
    ty.fields
        .sort_by_key(|field| span_key(field.name.span, document_order));
    ty.enum_values
        .sort_by_key(|value| span_key(value.name.span, document_order));
    ty.input_fields
        .sort_by_key(|field| span_key(field.name.span, document_order));
    rebuild_indices(ty);
}

fn reorder_directives(ty: &mut SemanticType, document_order: &BTreeMap<SourceId, usize>) {
    assert_eq!(
        ty.directives.len(),
        ty.directive_origins.len(),
        "The directive and the origin must be one-to-one."
    );
    let directives = std::mem::take(&mut ty.directives);
    let origins = std::mem::take(&mut ty.directive_origins);
    let mut entries = directives.into_iter().zip(origins).collect::<Vec<_>>();
    entries.sort_by_key(|(directive, _)| span_key(directive.span, document_order));
    (ty.directives, ty.directive_origins) = entries.into_iter().unzip();
}

fn rebuild_indices(ty: &mut SemanticType) {
    ty.field_index.clear();
    ty.field_argument_index.clear();
    for (index, field) in ty.fields.iter().enumerate() {
        ty.field_index.insert(field.name.value.clone(), index);
        ty.field_argument_index.insert(
            field.name.value.clone(),
            field
                .arguments
                .iter()
                .enumerate()
                .map(|(index, argument)| (argument.name.value.clone(), index))
                .collect(),
        );
    }
    ty.enum_value_index = ty
        .enum_values
        .iter()
        .enumerate()
        .map(|(index, value)| (value.name.value.clone(), index))
        .collect();
    ty.input_field_index = ty
        .input_fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.name.value.clone(), index))
        .collect();
}

fn origin_key(
    origin: SchemaOrigin,
    document_order: &BTreeMap<SourceId, usize>,
) -> (usize, usize, usize, usize) {
    origin.span().map_or((0, 0, 0, 0), |span| {
        let (document, start, end) = span_key(span, document_order);
        (1, document, start, end)
    })
}

fn span_key(span: Span, document_order: &BTreeMap<SourceId, usize>) -> (usize, usize, usize) {
    (
        document_order
            .get(&span.source_id)
            .copied()
            .unwrap_or(usize::MAX),
        span.start,
        span.end,
    )
}
