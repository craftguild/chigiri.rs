//! Validates the September 2025 subscription root-selection rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{
    Directive, Field, FragmentDefinition, OperationDefinition, Selection, SelectionSet,
};
use crate::schema::SemanticSchema;
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, Span};

pub(crate) const SUBSCRIPTION_SINGLE_ROOT_FIELD: DiagnosticCode = DiagnosticCode::new("SF3046");
pub(crate) const SUBSCRIPTION_ROOT_DIRECTIVE: DiagnosticCode = DiagnosticCode::new("SF3047");
pub(crate) const SUBSCRIPTION_INTROSPECTION_ROOT: DiagnosticCode = DiagnosticCode::new("SF3048");

type FragmentIndex<'document> = BTreeMap<&'document str, &'document FragmentDefinition>;

/// Collects root fields by response name without evaluating variables. This is
/// separate from runtime field collection because September 2025 Section
/// 5.2.4.1 forbids conditional directives in this traversal.
pub(super) fn validate<'document>(
    schema: &SemanticSchema,
    operation: &'document OperationDefinition,
    root_type: &str,
    fragments: &FragmentIndex<'document>,
    diagnostics: &mut DiagnosticBag,
) {
    let mut groups: BTreeMap<&str, Option<(&str, Span)>> = BTreeMap::new();
    let mut visited_fragments = BTreeSet::new();
    let mut work = reversed(&operation.selection_set);
    while let Some(selection) = work.pop() {
        reject_conditional_directives(selection_directives(selection), diagnostics);
        if diagnostics.should_stop() {
            return;
        }
        match selection {
            Selection::Field(field) => collect_field(&mut groups, field),
            Selection::FragmentSpread(spread) => {
                if !visited_fragments.insert(spread.name.value.as_str()) {
                    continue;
                }
                let Some(fragment) = fragments.get(spread.name.value.as_str()) else {
                    continue;
                };
                if applies(schema, root_type, &fragment.type_condition.value) {
                    push_reversed(&mut work, &fragment.selection_set);
                }
            }
            Selection::InlineFragment(fragment) => {
                let applies = fragment
                    .type_condition
                    .as_ref()
                    .is_none_or(|condition| applies(schema, root_type, &condition.value));
                if applies {
                    push_reversed(&mut work, &fragment.selection_set);
                }
            }
        }
    }
    if groups.len() != 1 {
        push(
            diagnostics,
            SUBSCRIPTION_SINGLE_ROOT_FIELD,
            "Subscription operation must have exactly one root field".to_owned(),
            operation.selection_set.span,
        );
        return;
    }
    if let Some((name, span)) = groups.values().next().copied().flatten() {
        push(
            diagnostics,
            SUBSCRIPTION_INTROSPECTION_ROOT,
            format!("`{name}` cannot be the root field of a subscription operation"),
            span,
        );
    }
}

fn collect_field<'document>(
    groups: &mut BTreeMap<&'document str, Option<(&'document str, Span)>>,
    field: &'document Field,
) {
    let response_name = field
        .alias
        .as_ref()
        .map_or(field.name.value.as_str(), |alias| alias.value.as_str());
    let introspection = field
        .name
        .value
        .starts_with("__")
        .then_some((field.name.value.as_str(), field.name.span));
    let entry = groups.entry(response_name).or_default();
    if entry.is_none() {
        *entry = introspection;
    }
}

fn applies(schema: &SemanticSchema, root_type: &str, condition: &str) -> bool {
    schema
        .possible_objects(condition)
        .is_some_and(|objects| objects.iter().any(|object| object.as_ref() == root_type))
}

fn selection_directives(selection: &Selection) -> &[Directive] {
    match selection {
        Selection::Field(field) => &field.directives,
        Selection::FragmentSpread(spread) => &spread.directives,
        Selection::InlineFragment(fragment) => &fragment.directives,
    }
}

fn reject_conditional_directives(directives: &[Directive], diagnostics: &mut DiagnosticBag) {
    for directive in directives {
        if matches!(directive.name.value.as_str(), "skip" | "include") {
            push(
                diagnostics,
                SUBSCRIPTION_ROOT_DIRECTIVE,
                format!(
                    "`@{}` is not allowed on a subscription root selection",
                    directive.name.value
                ),
                directive.span,
            );
            if diagnostics.should_stop() {
                return;
            }
        }
    }
}

fn reversed(selection_set: &SelectionSet) -> Vec<&Selection> {
    selection_set.selections.iter().rev().collect()
}

fn push_reversed<'document>(
    work: &mut Vec<&'document Selection>,
    selection_set: &'document SelectionSet,
) {
    work.extend(selection_set.selections.iter().rev());
}

fn push(diagnostics: &mut DiagnosticBag, code: DiagnosticCode, message: String, span: Span) {
    let _ = diagnostics.push(Diagnostic::new(code, message, span));
}
