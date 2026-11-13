//! Compares variable usage positions across each operation's reachable fragments.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{
    Definition, Directive, Document, ExecutableDefinition, Name, OperationDefinition, Selection,
    SelectionSet, Value as AstValue, ValueNode,
};
use crate::{Diagnostic, DiagnosticBag};

use super::VARIABLE_POSITION_INVALID;
use super::literal::{SiteMap, name_key};
use super::types::{InputType, compatible};

pub(super) fn validate_positions(
    document: &Document,
    sites: &SiteMap<'_>,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let fragments = fragment_index(document);
    let mut valid = true;
    for definition in &document.definitions {
        let Definition::Executable(ExecutableDefinition::Operation(operation)) = definition else {
            continue;
        };
        if !validate_operation(operation, &fragments, sites, diagnostics, &mut valid) {
            return false;
        }
    }
    valid
}

fn validate_operation(
    operation: &OperationDefinition,
    fragments: &BTreeMap<&str, &crate::language::ast::FragmentDefinition>,
    sites: &SiteMap<'_>,
    diagnostics: &mut DiagnosticBag,
    document_valid: &mut bool,
) -> bool {
    let definitions = operation
        .variable_definitions
        .iter()
        .map(|definition| (definition.variable.value.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    for usage in collect_uses(operation, fragments) {
        let (Some(definition), Some(site)) = (
            definitions.get(usage.value.as_str()),
            sites.get(&name_key(usage)),
        ) else {
            continue;
        };
        let variable_type = InputType::from_ast(&definition.ty);
        let non_null_default = definition
            .default_value
            .as_ref()
            .is_some_and(|value| !matches!(value.value, AstValue::Null));
        let allowed = if site.one_of_field {
            variable_type.is_non_null() && compatible(&variable_type, &site.expected, false)
        } else {
            compatible(
                &variable_type,
                &site.expected,
                non_null_default || site.location_default,
            )
        };
        if allowed {
            continue;
        }
        *document_valid = false;
        if !diagnostics.push(Diagnostic::new(
            VARIABLE_POSITION_INVALID,
            format!(
                "Variable `${}` is not allowed at this input position",
                usage.value
            ),
            usage.span,
        )) || diagnostics.should_stop()
        {
            return false;
        }
    }
    true
}

fn fragment_index(
    document: &Document,
) -> BTreeMap<&str, &crate::language::ast::FragmentDefinition> {
    document
        .definitions
        .iter()
        .filter_map(|definition| {
            let Definition::Executable(ExecutableDefinition::Fragment(fragment)) = definition
            else {
                return None;
            };
            Some((fragment.name.value.as_str(), fragment))
        })
        .collect()
}

enum Task<'a> {
    Selection(&'a SelectionSet),
    Directives(&'a [Directive]),
    Value(&'a ValueNode),
}

fn collect_uses<'a>(
    operation: &'a OperationDefinition,
    fragments: &BTreeMap<&str, &'a crate::language::ast::FragmentDefinition>,
) -> Vec<&'a Name> {
    let mut uses = Vec::new();
    let mut visited_fragments = BTreeSet::new();
    let mut work = vec![
        Task::Selection(&operation.selection_set),
        Task::Directives(&operation.directives),
    ];
    while let Some(task) = work.pop() {
        match task {
            Task::Directives(directives) => push_directives(&mut work, directives),
            Task::Value(value) => collect_value(&mut work, &mut uses, value),
            Task::Selection(set) => {
                for selection in set.selections.iter().rev() {
                    match selection {
                        Selection::Field(field) => {
                            if let Some(nested) = &field.selection_set {
                                work.push(Task::Selection(nested));
                            }
                            work.push(Task::Directives(&field.directives));
                            for argument in field.arguments.iter().rev() {
                                work.push(Task::Value(&argument.value));
                            }
                        }
                        Selection::InlineFragment(fragment) => {
                            work.push(Task::Selection(&fragment.selection_set));
                            work.push(Task::Directives(&fragment.directives));
                        }
                        Selection::FragmentSpread(spread) => {
                            if visited_fragments.insert(spread.name.value.as_str())
                                && let Some(fragment) = fragments.get(spread.name.value.as_str())
                            {
                                work.push(Task::Selection(&fragment.selection_set));
                                work.push(Task::Directives(&fragment.directives));
                            }
                            work.push(Task::Directives(&spread.directives));
                        }
                    }
                }
            }
        }
    }
    uses
}

fn push_directives<'a>(work: &mut Vec<Task<'a>>, directives: &'a [Directive]) {
    for directive in directives.iter().rev() {
        for argument in directive.arguments.iter().rev() {
            work.push(Task::Value(&argument.value));
        }
    }
}

fn collect_value<'a>(work: &mut Vec<Task<'a>>, uses: &mut Vec<&'a Name>, value: &'a ValueNode) {
    match &value.value {
        AstValue::Variable(name) => uses.push(name),
        AstValue::List(items) => work.extend(items.iter().rev().map(Task::Value)),
        AstValue::Object(fields) => {
            work.extend(fields.iter().rev().map(|field| Task::Value(&field.value)));
        }
        AstValue::Int(_)
        | AstValue::Float(_)
        | AstValue::String(_)
        | AstValue::Boolean(_)
        | AstValue::Null
        | AstValue::Enum(_) => {}
    }
}
