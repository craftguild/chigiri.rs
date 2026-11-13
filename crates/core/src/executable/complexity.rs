//! Summarize the fragment DAG once and total operation resource use.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{
    Definition, Document, ExecutableDefinition, OperationDefinition, Selection, SelectionSet,
};
use crate::{Diagnostic, DiagnosticBag, ExecutionLimits, Span};

use super::{
    ComplexitySummary, EXECUTABLE_ALIAS_LIMIT, EXECUTABLE_DEPTH_LIMIT, EXECUTABLE_DIRECTIVE_LIMIT,
    EXECUTABLE_FIELD_LIMIT,
};

#[derive(Clone, Debug, Default)]
struct LocalSummary {
    summary: ComplexitySummary,
    edges: Vec<Edge>,
}

#[derive(Clone, Debug)]
struct Edge {
    fragment: String,
    depth_offset: usize,
}

pub(super) fn summarize(
    document: &Document,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> (BTreeMap<usize, ComplexitySummary>, bool) {
    let fragments = fragment_locals(document);
    let summaries = resolve_fragments(&fragments);
    let mut operations = BTreeMap::new();
    let mut valid = true;
    for (index, definition) in document.definitions.iter().enumerate() {
        let Definition::Executable(ExecutableDefinition::Operation(operation)) = definition else {
            continue;
        };
        let mut local = selection_local(&operation.selection_set);
        local.summary.variables = operation.variable_definitions.len();
        local.summary.directives = local
            .summary
            .directives
            .saturating_add(operation.directives.len());
        for variable in &operation.variable_definitions {
            local.summary.directives = local
                .summary
                .directives
                .saturating_add(variable.directives.len());
        }
        let summary = compose(local, &summaries);
        if !check_limits(operation, summary, limits, diagnostics) {
            valid = false;
            if diagnostics.should_stop() {
                break;
            }
        }
        operations.insert(index, summary);
    }
    (operations, valid)
}

fn fragment_locals(document: &Document) -> BTreeMap<String, LocalSummary> {
    document
        .definitions
        .iter()
        .filter_map(|definition| {
            let Definition::Executable(ExecutableDefinition::Fragment(fragment)) = definition
            else {
                return None;
            };
            let mut local = selection_local(&fragment.selection_set);
            local.summary.directives = local
                .summary
                .directives
                .saturating_add(fragment.directives.len());
            Some((fragment.name.value.clone(), local))
        })
        .collect()
}

fn selection_local(root: &SelectionSet) -> LocalSummary {
    let mut local = LocalSummary::default();
    let mut work = vec![(root, 0_usize)];
    while let Some((set, parent_depth)) = work.pop() {
        for selection in &set.selections {
            match selection {
                Selection::Field(field) => {
                    let field_depth = parent_depth.saturating_add(1);
                    local.summary.depth = local.summary.depth.max(field_depth);
                    local.summary.expanded_fields = local.summary.expanded_fields.saturating_add(1);
                    local.summary.aliases = local
                        .summary
                        .aliases
                        .saturating_add(usize::from(field.alias.is_some()));
                    local.summary.directives = local
                        .summary
                        .directives
                        .saturating_add(field.directives.len());
                    if let Some(nested) = &field.selection_set {
                        work.push((nested, field_depth));
                    }
                }
                Selection::InlineFragment(fragment) => {
                    local.summary.directives = local
                        .summary
                        .directives
                        .saturating_add(fragment.directives.len());
                    work.push((&fragment.selection_set, parent_depth));
                }
                Selection::FragmentSpread(spread) => {
                    local.summary.directives = local
                        .summary
                        .directives
                        .saturating_add(spread.directives.len());
                    local.edges.push(Edge {
                        fragment: spread.name.value.clone(),
                        depth_offset: parent_depth,
                    });
                }
            }
        }
    }
    local
}

fn resolve_fragments(
    locals: &BTreeMap<String, LocalSummary>,
) -> BTreeMap<String, ComplexitySummary> {
    let mut unresolved = BTreeMap::new();
    let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, local) in locals {
        let dependencies = local
            .edges
            .iter()
            .filter_map(|edge| {
                locals
                    .contains_key(&edge.fragment)
                    .then_some(edge.fragment.as_str())
            })
            .collect::<BTreeSet<_>>();
        unresolved.insert(name.as_str(), dependencies.len());
        for dependency in dependencies {
            dependents.entry(dependency).or_default().insert(name);
        }
    }
    let mut ready = unresolved
        .iter()
        .filter_map(|(name, count)| (*count == 0).then_some(*name))
        .collect::<BTreeSet<_>>();
    let mut summaries = BTreeMap::new();
    while let Some(name) = ready.pop_first() {
        let summary = compose(locals[name].clone(), &summaries);
        summaries.insert(name.to_owned(), summary);
        if let Some(waiting) = dependents.get(name) {
            for dependent in waiting {
                let count = unresolved
                    .get_mut(dependent)
                    .expect("fragment must be known after validation");
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent);
                }
            }
        }
    }
    summaries
}

fn compose(
    local: LocalSummary,
    fragments: &BTreeMap<String, ComplexitySummary>,
) -> ComplexitySummary {
    let mut summary = local.summary;
    for edge in local.edges {
        let Some(fragment) = fragments.get(&edge.fragment) else {
            continue;
        };
        summary.depth = summary
            .depth
            .max(edge.depth_offset.saturating_add(fragment.depth));
        summary.expanded_fields = summary
            .expanded_fields
            .saturating_add(fragment.expanded_fields);
        summary.aliases = summary.aliases.saturating_add(fragment.aliases);
        summary.directives = summary.directives.saturating_add(fragment.directives);
    }
    summary
}

fn check_limits(
    operation: &OperationDefinition,
    summary: ComplexitySummary,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let mut valid = true;
    let checks = [
        (
            summary.depth,
            limits.max_operation_depth,
            EXECUTABLE_DEPTH_LIMIT,
            "selection depth",
        ),
        (
            summary.expanded_fields,
            limits.max_expanded_fields,
            EXECUTABLE_FIELD_LIMIT,
            "expanded field count",
        ),
        (
            summary.aliases,
            limits.max_aliases,
            EXECUTABLE_ALIAS_LIMIT,
            "alias count",
        ),
        (
            summary.directives,
            limits.max_directives,
            EXECUTABLE_DIRECTIVE_LIMIT,
            "directive count",
        ),
    ];
    for (actual, limit, code, label) in checks {
        if actual <= limit {
            continue;
        }
        valid = false;
        if !report_limit(diagnostics, code, label, actual, limit, operation.span) {
            break;
        }
    }
    valid
}

fn report_limit(
    diagnostics: &mut DiagnosticBag,
    code: crate::DiagnosticCode,
    label: &str,
    actual: usize,
    limit: usize,
    span: Span,
) -> bool {
    diagnostics.push(Diagnostic::new(
        code,
        format!("Operation {label} {actual} exceeds configured limit {limit}"),
        span,
    )) && !diagnostics.should_stop()
}
