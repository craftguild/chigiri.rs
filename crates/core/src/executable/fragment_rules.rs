//! Validate fragment type conditions, spread graphs, and reachability.

mod graph;
mod possible_types;

use crate::language::ast::{
    Definition, Document, ExecutableDefinition, OperationType, Selection, SelectionSet, TypeRef,
    TypeRefKind,
};
use crate::schema::{SemanticSchema, TypeKind};
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, Span};

use graph::{FragmentCatalog, SpreadGraph};
use possible_types::PossibleTypes;

/// A type condition for a fragment or inline fragment is not a composite type.
pub(super) const FRAGMENT_INVALID_TYPE_CONDITION: DiagnosticCode = DiagnosticCode::new("SF3010");
/// A named fragment spread's target does not exist in the document.
pub(super) const FRAGMENT_SPREAD_TARGET_UNDEFINED: DiagnosticCode = DiagnosticCode::new("SF3011");
/// The fragment spread graph contains a cycle.
pub(super) const FRAGMENT_SPREAD_CYCLE: DiagnosticCode = DiagnosticCode::new("SF3012");
/// The fragment cannot be reached by any operation.
pub(super) const FRAGMENT_UNUSED: DiagnosticCode = DiagnosticCode::new("SF3013");
/// The runtime object candidates for the spread location and type condition do not intersect.
pub(super) const FRAGMENT_SPREAD_NOT_POSSIBLE: DiagnosticCode = DiagnosticCode::new("SF3014");

struct WalkContext<'a, 'document> {
    schema: &'document SemanticSchema,
    catalog: &'a FragmentCatalog<'document>,
    possible_types: &'a PossibleTypes<'a>,
    diagnostics: &'a mut DiagnosticBag,
    graph: &'a mut SpreadGraph,
    owner: usize,
    valid: &'a mut bool,
}

/// Validates the fragment-specific rules in Section 5.5 with bounded,
/// non-recursive traversal.
pub(super) fn validate_fragments(
    schema: &SemanticSchema,
    document: &Document,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let catalog = FragmentCatalog::new(document);
    let possible_types = PossibleTypes::new(schema);
    let mut graph = SpreadGraph::new(catalog.len());
    let mut valid = true;

    for definition in &document.definitions {
        let Definition::Executable(definition) = definition else {
            continue;
        };
        let (selection_set, parent_type, owner) = match definition {
            ExecutableDefinition::Operation(operation) => (
                &operation.selection_set,
                operation_root(schema, operation.operation_type),
                graph.operation_node(),
            ),
            ExecutableDefinition::Fragment(fragment) => {
                let Some(index) = catalog.index(fragment.name.value.as_str()) else {
                    continue;
                };
                // The over-definition only treats the first as a graph node.
                if !catalog.is_primary(index, fragment) {
                    continue;
                }
                let parent = match valid_condition(
                    schema,
                    &fragment.type_condition.value,
                    fragment.type_condition.span,
                    diagnostics,
                ) {
                    Ok(parent) => parent,
                    Err(()) => return false,
                };
                if parent.is_none() {
                    valid = false;
                }
                (&fragment.selection_set, parent, index)
            }
        };
        let mut context = WalkContext {
            schema,
            catalog: &catalog,
            possible_types: &possible_types,
            diagnostics,
            graph: &mut graph,
            owner,
            valid: &mut valid,
        };
        if !walk_selection_set(selection_set, parent_type, &mut context) {
            return false;
        }
    }
    let graph_valid = graph.validate(&catalog, diagnostics);
    valid && graph_valid
}

fn walk_selection_set<'document>(
    selection_set: &'document SelectionSet,
    parent_type: Option<&'document str>,
    context: &mut WalkContext<'_, 'document>,
) -> bool {
    let mut jobs = Vec::new();
    push_selections(&mut jobs, selection_set, parent_type);
    while let Some(job) = jobs.pop() {
        match job.0 {
            Selection::Field(field) => {
                let Some(children) = &field.selection_set else {
                    continue;
                };
                let child_type = job
                    .1
                    .and_then(|parent| field_named_type(context.schema, parent, &field.name.value));
                push_selections(&mut jobs, children, child_type);
            }
            Selection::FragmentSpread(spread) => {
                let Some(target) = context.catalog.index(spread.name.value.as_str()) else {
                    *context.valid = false;
                    if !push_diagnostic(
                        context.diagnostics,
                        FRAGMENT_SPREAD_TARGET_UNDEFINED,
                        format!("Fragment `{}` is not defined", spread.name.value),
                        spread.name.span,
                    ) {
                        return false;
                    }
                    continue;
                };
                context.graph.add(context.owner, target, spread.span);
                let condition = &context.catalog.definition(target).type_condition.value;
                if !validate_overlap(job.1, condition, spread.span, context) {
                    return false;
                }
            }
            Selection::InlineFragment(fragment) => {
                let child_type = match &fragment.type_condition {
                    Some(condition) => {
                        let valid = match valid_condition(
                            context.schema,
                            &condition.value,
                            condition.span,
                            context.diagnostics,
                        ) {
                            Ok(valid) => valid,
                            Err(()) => return false,
                        };
                        if valid.is_none() {
                            *context.valid = false;
                        }
                        if valid.is_some()
                            && !validate_overlap(job.1, &condition.value, fragment.span, context)
                        {
                            return false;
                        }
                        valid
                    }
                    None => job.1,
                };
                push_selections(&mut jobs, &fragment.selection_set, child_type);
            }
        }
    }
    true
}

fn push_selections<'document>(
    jobs: &mut Vec<(&'document Selection, Option<&'document str>)>,
    selection_set: &'document SelectionSet,
    parent_type: Option<&'document str>,
) {
    jobs.extend(
        selection_set
            .selections
            .iter()
            .rev()
            .map(|selection| (selection, parent_type)),
    );
}

fn valid_condition<'a>(
    schema: &SemanticSchema,
    name: &'a str,
    span: Span,
    diagnostics: &mut DiagnosticBag,
) -> Result<Option<&'a str>, ()> {
    let valid = matches!(
        schema.type_by_name(name).map(|ty| ty.kind()),
        Some(TypeKind::Object | TypeKind::Interface | TypeKind::Union)
    ) || crate::schema::introspection::type_by_name(name)
        .is_some_and(|ty| ty.kind == crate::schema::introspection::model::DescriptorKind::Object);
    if valid {
        return Ok(Some(name));
    }
    if push_diagnostic(
        diagnostics,
        FRAGMENT_INVALID_TYPE_CONDITION,
        format!("Fragment type condition `{name}` must be a defined composite type"),
        span,
    ) {
        Ok(None)
    } else {
        Err(())
    }
}

fn validate_overlap(
    parent: Option<&str>,
    condition: &str,
    span: Span,
    context: &mut WalkContext<'_, '_>,
) -> bool {
    let Some(parent) = parent else {
        return true;
    };
    if context.possible_types.overlap(parent, condition) != Some(false) {
        return true;
    }
    *context.valid = false;
    push_diagnostic(
        context.diagnostics,
        FRAGMENT_SPREAD_NOT_POSSIBLE,
        format!("Fragment on `{condition}` cannot be spread within `{parent}`"),
        span,
    )
}

fn operation_root(schema: &SemanticSchema, operation: OperationType) -> Option<&str> {
    let root = match operation {
        OperationType::Query => schema.roots().query(),
        OperationType::Mutation => schema.roots().mutation(),
        OperationType::Subscription => schema.roots().subscription(),
    }?;
    Some(&root.type_name)
}

fn named_type(reference: &TypeRef) -> &str {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner,
        }
    }
}

fn field_named_type<'a>(schema: &'a SemanticSchema, parent: &str, field: &str) -> Option<&'a str> {
    if let Some(definition) = schema.type_by_name(parent).and_then(|ty| ty.field(field)) {
        return Some(named_type(&definition.ty));
    }
    if schema
        .roots()
        .query()
        .is_some_and(|root| root.type_name == parent)
    {
        match field {
            "__schema" => return Some("__Schema"),
            "__type" => return Some("__Type"),
            _ => {}
        }
    }
    crate::schema::introspection::type_by_name(parent)?
        .fields
        .iter()
        .find(|definition| definition.name == field)
        .map(|definition| definition.ty.named_type)
}

fn push_diagnostic(
    diagnostics: &mut DiagnosticBag,
    code: DiagnosticCode,
    message: impl Into<String>,
    span: Span,
) -> bool {
    diagnostics.push(Diagnostic::new(code, message, span)) && !diagnostics.should_stop()
}
