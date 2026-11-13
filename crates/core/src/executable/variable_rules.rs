//! Validate variable definitions and reachable uses for each operation.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{
    Definition, Directive, Document, ExecutableDefinition, Name, OperationDefinition, Selection,
    SelectionSet, TypeRef, TypeRefKind, Value as AstValue, ValueNode,
};
use crate::schema::{SemanticSchema, TypeKind};
use crate::{Diagnostic, DiagnosticBag, ExecutionLimits, Span};

use super::{
    EXECUTABLE_DUPLICATE_VARIABLE, EXECUTABLE_VARIABLE_LIMIT, EXECUTABLE_VARIABLE_NOT_INPUT,
    EXECUTABLE_VARIABLE_UNDEFINED, EXECUTABLE_VARIABLE_UNUSED,
};

pub(super) fn validate_variables(
    schema: &SemanticSchema,
    document: &Document,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let fragments = fragment_index(document);
    let mut valid = true;
    for definition in &document.definitions {
        let Definition::Executable(ExecutableDefinition::Operation(operation)) = definition else {
            continue;
        };
        if !validate_operation(schema, operation, &fragments, limits, diagnostics) {
            valid = false;
            if diagnostics.should_stop() {
                break;
            }
        }
    }
    valid
}

fn validate_operation(
    schema: &SemanticSchema,
    operation: &OperationDefinition,
    fragments: &BTreeMap<&str, &crate::language::ast::FragmentDefinition>,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let mut valid = true;
    if operation.variable_definitions.len() > limits.max_variables {
        valid = false;
        if !report(
            diagnostics,
            EXECUTABLE_VARIABLE_LIMIT,
            format!(
                "Operation defines more than {} variables",
                limits.max_variables
            ),
            operation.span,
        ) {
            return false;
        }
    }
    let mut definitions = BTreeMap::new();
    for definition in &operation.variable_definitions {
        if definitions
            .insert(definition.variable.value.as_str(), definition)
            .is_some()
        {
            valid = false;
            if !report(
                diagnostics,
                EXECUTABLE_DUPLICATE_VARIABLE,
                format!(
                    "Variable `${}` is defined more than once",
                    definition.variable.value
                ),
                definition.variable.span,
            ) {
                return false;
            }
        }
        if !is_input_type(schema, &definition.ty) {
            valid = false;
            if !report(
                diagnostics,
                EXECUTABLE_VARIABLE_NOT_INPUT,
                format!(
                    "Variable `${}` must use a defined input type",
                    definition.variable.value
                ),
                definition.ty.span,
            ) {
                return false;
            }
        }
    }
    let uses = collect_uses(operation, fragments);
    let mut used = BTreeSet::new();
    for usage in uses {
        used.insert(usage.value.as_str());
        if !definitions.contains_key(usage.value.as_str()) {
            valid = false;
            if !report(
                diagnostics,
                EXECUTABLE_VARIABLE_UNDEFINED,
                format!(
                    "Variable `${}` is not defined by this operation",
                    usage.value
                ),
                usage.span,
            ) {
                return false;
            }
        }
    }
    for definition in &operation.variable_definitions {
        if !used.contains(definition.variable.value.as_str()) {
            valid = false;
            if !report(
                diagnostics,
                EXECUTABLE_VARIABLE_UNUSED,
                format!("Variable `${}` is never used", definition.variable.value),
                definition.variable.span,
            ) {
                return false;
            }
        }
    }
    valid
}

fn is_input_type(schema: &SemanticSchema, reference: &TypeRef) -> bool {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => {
                return schema.type_by_name(&name.value).is_some_and(|named| {
                    matches!(
                        named.kind(),
                        TypeKind::Scalar | TypeKind::Enum | TypeKind::InputObject
                    )
                });
            }
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner,
        }
    }
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
    for variable in operation.variable_definitions.iter().rev() {
        work.push(Task::Directives(&variable.directives));
    }
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
        AstValue::List(items) => {
            work.extend(items.iter().rev().map(Task::Value));
        }
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

fn report(
    diagnostics: &mut DiagnosticBag,
    code: crate::DiagnosticCode,
    message: impl Into<String>,
    span: Span,
) -> bool {
    diagnostics.push(Diagnostic::new(code, message, span)) && !diagnostics.should_stop()
}
