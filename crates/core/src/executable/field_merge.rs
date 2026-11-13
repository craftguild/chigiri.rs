//! Validate field-selection merging with iterative fragment expansion.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::language::ast::{
    Definition, Document, ExecutableDefinition, Field, FragmentDefinition, Selection, SelectionSet,
};
use crate::schema::SemanticSchema;
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode};

use super::field_merge_values::field_signature;
use super::field_support::{OutputType, PossibleObjects, field_output, is_concrete_object};

pub(super) const MERGE_RETURN_SHAPE: DiagnosticCode = DiagnosticCode::new("SF3036");
pub(super) const MERGE_FIELD_NAME: DiagnosticCode = DiagnosticCode::new("SF3037");
pub(super) const MERGE_ARGUMENTS: DiagnosticCode = DiagnosticCode::new("SF3038");
#[derive(Clone)]
struct Scope<'a> {
    set: &'a SelectionSet,
    parent: &'a str,
    possible: Arc<[Arc<str>]>,
}

struct Occurrence<'a> {
    field: &'a Field,
    output: OutputType<'a>,
    concrete_parent: Option<&'a str>,
}

enum MergeWork<'a> {
    Unit(Vec<Scope<'a>>),
    Group(Vec<Occurrence<'a>>),
}

pub(super) fn validate_merges<'a>(
    schema: &'a SemanticSchema,
    document: &'a Document,
    fragments: &BTreeMap<&'a str, &'a FragmentDefinition>,
    diagnostics: &mut DiagnosticBag,
) {
    let possible_objects = PossibleObjects::new(schema);
    let mut work = Vec::new();
    for definition in document.definitions.iter().rev() {
        let Definition::Executable(definition) = definition else {
            continue;
        };
        let initial = match definition {
            ExecutableDefinition::Operation(operation) => {
                let root = match operation.operation_type {
                    crate::language::ast::OperationType::Query => schema.roots().query(),
                    crate::language::ast::OperationType::Mutation => schema.roots().mutation(),
                    crate::language::ast::OperationType::Subscription => {
                        schema.roots().subscription()
                    }
                };
                let Some(root) = root else {
                    continue;
                };
                scope(&possible_objects, &operation.selection_set, &root.type_name)
            }
            ExecutableDefinition::Fragment(fragment) => scope(
                &possible_objects,
                &fragment.selection_set,
                &fragment.type_condition.value,
            ),
        };
        work.push(MergeWork::Unit(vec![initial]));
    }
    while let Some(item) = work.pop() {
        if diagnostics.should_stop() {
            return;
        }
        match item {
            MergeWork::Unit(unit) => {
                let occurrences = collect_fields(schema, &possible_objects, fragments, &unit);
                for group in group_by_response_key(occurrences).into_iter().rev() {
                    work.push(MergeWork::Group(group));
                }
            }
            MergeWork::Group(group) => {
                validate_group(&group, diagnostics);
                if let Some(nested) = nested_unit(&possible_objects, &group) {
                    work.push(MergeWork::Unit(nested));
                }
            }
        }
    }
}

fn scope<'a>(
    possible_objects: &PossibleObjects,
    set: &'a SelectionSet,
    parent: &'a str,
) -> Scope<'a> {
    Scope {
        set,
        parent,
        possible: possible_objects.get(parent),
    }
}

enum Work<'a> {
    Set(Scope<'a>),
    Selection(&'a Selection, &'a str, Arc<[Arc<str>]>),
}

fn collect_fields<'a>(
    schema: &'a SemanticSchema,
    possible_objects: &PossibleObjects,
    fragments: &BTreeMap<&'a str, &'a FragmentDefinition>,
    roots: &[Scope<'a>],
) -> Vec<Occurrence<'a>> {
    let mut fields = Vec::new();
    let mut work = roots
        .iter()
        .rev()
        .cloned()
        .map(Work::Set)
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut conditioned = BTreeMap::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Set(scope) => {
                let key = (
                    scope.set as *const SelectionSet as usize,
                    scope.parent,
                    scope.possible.clone(),
                );
                if !visited.insert(key) {
                    continue;
                }
                for selection in scope.set.selections.iter().rev() {
                    work.push(Work::Selection(
                        selection,
                        scope.parent,
                        scope.possible.clone(),
                    ));
                }
            }
            Work::Selection(selection, parent, possible) => match selection {
                Selection::Field(field) => {
                    if let Some(output) = field_output(schema, parent, field) {
                        fields.push(Occurrence {
                            field,
                            output,
                            concrete_parent: is_concrete_object(schema, parent).then_some(parent),
                        });
                    }
                }
                Selection::InlineFragment(fragment) => {
                    let target = fragment
                        .type_condition
                        .as_ref()
                        .map_or(parent, |name| name.value.as_str());
                    push_conditioned(
                        possible_objects,
                        &mut conditioned,
                        &mut work,
                        &fragment.selection_set,
                        target,
                        &possible,
                    );
                }
                Selection::FragmentSpread(spread) => {
                    if let Some(fragment) = fragments.get(spread.name.value.as_str()) {
                        push_conditioned(
                            possible_objects,
                            &mut conditioned,
                            &mut work,
                            &fragment.selection_set,
                            &fragment.type_condition.value,
                            &possible,
                        );
                    }
                }
            },
        }
    }
    fields
}

fn push_conditioned<'a>(
    possible_objects: &PossibleObjects,
    cache: &mut ConditionCache<'a>,
    work: &mut Vec<Work<'a>>,
    set: &'a SelectionSet,
    target: &'a str,
    current: &Arc<[Arc<str>]>,
) {
    if current.is_empty() {
        return;
    }
    let key = (current.as_ptr() as usize, target);
    let possible = cache.get(&key).map(|entry| entry.1.clone()).or_else(|| {
        let possible = possible_objects.intersect(current, target)?;
        cache.insert(key, (current.clone(), possible.clone()));
        Some(possible)
    });
    let Some(possible) = possible else {
        return;
    };
    if !possible.is_empty() {
        work.push(Work::Set(Scope {
            set,
            parent: target,
            possible,
        }));
    }
}

type ConditionCache<'a> = BTreeMap<(usize, &'a str), (Arc<[Arc<str>]>, Arc<[Arc<str>]>)>;

fn group_by_response_key<'a>(occurrences: Vec<Occurrence<'a>>) -> Vec<Vec<Occurrence<'a>>> {
    let mut indices = BTreeMap::new();
    let mut groups: Vec<Vec<Occurrence<'a>>> = Vec::new();
    for occurrence in occurrences {
        let key = occurrence
            .field
            .alias
            .as_ref()
            .map_or(&occurrence.field.name.value, |alias| &alias.value);
        let index = *indices.entry(key.clone()).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[index].push(occurrence);
    }
    groups
}

fn validate_group(group: &[Occurrence<'_>], diagnostics: &mut DiagnosticBag) {
    let first = &group[0];
    for occurrence in group.iter().skip(1) {
        if !first.output.has_same_shape(&occurrence.output) {
            push(
                diagnostics,
                MERGE_RETURN_SHAPE,
                "Fields with the same response key must have compatible return shapes",
                occurrence.field.span,
            );
            if diagnostics.should_stop() {
                return;
            }
        }
    }
    validate_overlapping_signatures(group, diagnostics);
}

fn validate_overlapping_signatures(group: &[Occurrence<'_>], diagnostics: &mut DiagnosticBag) {
    let signatures = group
        .iter()
        .map(|occurrence| field_signature(occurrence.field))
        .collect::<Vec<_>>();
    if signatures.windows(2).all(|pair| pair[0] == pair[1]) {
        return;
    }
    let abstract_parent = group
        .iter()
        .position(|occurrence| occurrence.concrete_parent.is_none());
    let mut concrete: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, occurrence) in group.iter().enumerate() {
        let previous = abstract_parent.or_else(|| {
            let parent = occurrence.concrete_parent?;
            concrete.insert(parent, index)
        });
        let Some(previous) = previous.filter(|previous| signatures[*previous] != signatures[index])
        else {
            continue;
        };
        let code = if group[previous].field.name.value != occurrence.field.name.value {
            MERGE_FIELD_NAME
        } else {
            MERGE_ARGUMENTS
        };
        push(
            diagnostics,
            code,
            "Overlapping fields with the same response key must use the same field and arguments",
            occurrence.field.span,
        );
        if diagnostics.should_stop() {
            return;
        }
    }
}

fn nested_unit<'a>(
    possible_objects: &PossibleObjects,
    group: &[Occurrence<'a>],
) -> Option<Vec<Scope<'a>>> {
    let nested = group
        .iter()
        .filter_map(|occurrence| {
            occurrence
                .field
                .selection_set
                .as_ref()
                .filter(|_| !occurrence.output.is_leaf)
                .map(|set| scope(possible_objects, set, occurrence.output.named))
        })
        .collect::<Vec<_>>();
    (!nested.is_empty()).then_some(nested)
}

fn push(diagnostics: &mut DiagnosticBag, code: DiagnosticCode, message: &str, span: crate::Span) {
    let _ = diagnostics.push(Diagnostic::new(code, message, span));
}
