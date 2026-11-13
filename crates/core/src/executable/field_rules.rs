//! Validates schema-dependent field-selection and executable-directive rules.

#[path = "directive_rules.rs"]
mod directive_rules;
#[path = "field_merge.rs"]
mod field_merge;
#[path = "field_merge_values.rs"]
mod field_merge_values;
#[path = "field_support.rs"]
mod field_support;
#[path = "subscription_rules.rs"]
mod subscription_rules;
#[cfg(test)]
#[path = "tests_type_names.rs"]
mod tests_type_names;

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{
    Definition, DirectiveLocation, Document, ExecutableDefinition, Field, Selection, SelectionSet,
};
use crate::schema::SemanticSchema;
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode};

use directive_rules::validate_directives;
use field_support::{
    FieldArgumentCatalog, field_output, is_composite, operation_location, root_name,
};
pub(super) use subscription_rules::{
    SUBSCRIPTION_INTROSPECTION_ROOT, SUBSCRIPTION_ROOT_DIRECTIVE, SUBSCRIPTION_SINGLE_ROOT_FIELD,
};

pub(super) const ROOT_TYPE_MISSING: DiagnosticCode = DiagnosticCode::new("SF3030");
pub(super) const FIELD_NOT_DEFINED: DiagnosticCode = DiagnosticCode::new("SF3031");
pub(super) const FIELD_SELECTION_REQUIRED: DiagnosticCode = DiagnosticCode::new("SF3032");
pub(super) const LEAF_SELECTION_PRESENT: DiagnosticCode = DiagnosticCode::new("SF3033");
pub(super) const FIELD_ARGUMENT_UNKNOWN: DiagnosticCode = DiagnosticCode::new("SF3034");
pub(super) const FIELD_ARGUMENT_DUPLICATE: DiagnosticCode = DiagnosticCode::new("SF3035");
pub(super) const FIELD_ARGUMENT_REQUIRED: DiagnosticCode = DiagnosticCode::new("SF3039");

/// Validates document fields, arguments, and directives in source order.
pub(super) fn validate_fields(
    schema: &SemanticSchema,
    document: &Document,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let initial_len = diagnostics.len();
    let initially_truncated = diagnostics.was_truncated();
    validate_fields_inner(schema, document, diagnostics);
    diagnostics.len() == initial_len && diagnostics.was_truncated() == initially_truncated
}

fn validate_fields_inner(
    schema: &SemanticSchema,
    document: &Document,
    diagnostics: &mut DiagnosticBag,
) {
    let mut fields = FieldArgumentCatalog::new(schema);
    let fragments = fragment_index(document);
    for definition in &document.definitions {
        if diagnostics.should_stop() {
            return;
        }
        let Definition::Executable(definition) = definition else {
            continue;
        };
        match definition {
            ExecutableDefinition::Operation(operation) => {
                for variable in &operation.variable_definitions {
                    validate_directives(
                        schema,
                        &variable.directives,
                        DirectiveLocation::VariableDefinition,
                        diagnostics,
                    );
                }
                let location = operation_location(operation.operation_type);
                validate_directives(schema, &operation.directives, location, diagnostics);
                let Some(root) = root_name(schema, operation.operation_type) else {
                    push(
                        diagnostics,
                        ROOT_TYPE_MISSING,
                        format!("No root type is defined for {:?}", operation.operation_type),
                        operation.span,
                    );
                    continue;
                };
                walk_selection_set(
                    schema,
                    &mut fields,
                    &operation.selection_set,
                    root,
                    diagnostics,
                );
                if operation.operation_type == crate::language::ast::OperationType::Subscription
                    && !diagnostics.should_stop()
                {
                    subscription_rules::validate(schema, operation, root, &fragments, diagnostics);
                }
            }
            ExecutableDefinition::Fragment(fragment) => {
                validate_directives(
                    schema,
                    &fragment.directives,
                    DirectiveLocation::FragmentDefinition,
                    diagnostics,
                );
                if is_composite(schema, &fragment.type_condition.value) {
                    walk_selection_set(
                        schema,
                        &mut fields,
                        &fragment.selection_set,
                        &fragment.type_condition.value,
                        diagnostics,
                    );
                }
            }
        }
    }
    if !diagnostics.should_stop() {
        field_merge::validate_merges(schema, document, &fragments, diagnostics);
    }
}

fn walk_selection_set(
    schema: &SemanticSchema,
    fields: &mut FieldArgumentCatalog<'_>,
    root: &SelectionSet,
    root_parent: &str,
    diagnostics: &mut DiagnosticBag,
) {
    let mut work = root
        .selections
        .iter()
        .rev()
        .map(|selection| (selection, root_parent))
        .collect::<Vec<_>>();
    while let Some((selection, parent)) = work.pop() {
        if diagnostics.should_stop() {
            return;
        }
        match selection {
            Selection::Field(field) => {
                validate_field(schema, fields, field, parent, diagnostics);
                if let Some(nested) = &field.selection_set
                    && let Some(output) = field_output(schema, parent, field)
                    && !output.is_leaf
                {
                    for selection in nested.selections.iter().rev() {
                        work.push((selection, output.named));
                    }
                }
            }
            Selection::FragmentSpread(spread) => validate_directives(
                schema,
                &spread.directives,
                DirectiveLocation::FragmentSpread,
                diagnostics,
            ),
            Selection::InlineFragment(fragment) => {
                validate_directives(
                    schema,
                    &fragment.directives,
                    DirectiveLocation::InlineFragment,
                    diagnostics,
                );
                let next = fragment
                    .type_condition
                    .as_ref()
                    .map_or(parent, |name| name.value.as_str());
                if is_composite(schema, next) {
                    for selection in fragment.selection_set.selections.iter().rev() {
                        work.push((selection, next));
                    }
                }
            }
        }
    }
}

fn validate_field(
    schema: &SemanticSchema,
    fields: &mut FieldArgumentCatalog<'_>,
    field: &Field,
    parent: &str,
    diagnostics: &mut DiagnosticBag,
) {
    validate_directives(
        schema,
        &field.directives,
        DirectiveLocation::Field,
        diagnostics,
    );
    if diagnostics.should_stop() {
        return;
    }
    let Some(output) = field_output(schema, parent, field) else {
        push(
            diagnostics,
            FIELD_NOT_DEFINED,
            format!("Field `{}.{}` is not defined", parent, field.name.value),
            field.name.span,
        );
        return;
    };
    validate_field_arguments(fields, parent, field, diagnostics);
    match (output.is_leaf, field.selection_set.as_ref()) {
        (true, Some(set)) => push(
            diagnostics,
            LEAF_SELECTION_PRESENT,
            format!(
                "Leaf field `{}.{}` cannot have a selection set",
                parent, field.name.value
            ),
            set.span,
        ),
        (false, None) => push(
            diagnostics,
            FIELD_SELECTION_REQUIRED,
            format!(
                "Composite field `{}.{}` requires a selection set",
                parent, field.name.value
            ),
            field.span,
        ),
        _ => {}
    }
}

fn validate_field_arguments(
    catalog: &mut FieldArgumentCatalog<'_>,
    parent: &str,
    field: &Field,
    diagnostics: &mut DiagnosticBag,
) {
    let definitions = catalog.get(parent, &field.name.value);
    let mut supplied = BTreeSet::new();
    for argument in &field.arguments {
        if diagnostics.should_stop() {
            return;
        }
        if !supplied.insert(argument.name.value.as_str()) {
            push(
                diagnostics,
                FIELD_ARGUMENT_DUPLICATE,
                format!(
                    "Argument `{}` is supplied more than once",
                    argument.name.value
                ),
                argument.name.span,
            );
        }
        if diagnostics.should_stop() {
            return;
        }
        if !definitions.is_some_and(|rule| rule.arguments.contains(argument.name.value.as_str())) {
            push(
                diagnostics,
                FIELD_ARGUMENT_UNKNOWN,
                format!(
                    "Argument `{}` is not defined on `{}.{}`",
                    argument.name.value, parent, field.name.value
                ),
                argument.name.span,
            );
        }
    }
    for name in definitions.into_iter().flat_map(|rule| &rule.required) {
        if diagnostics.should_stop() {
            return;
        }
        if !supplied.contains(name.as_str()) {
            push(
                diagnostics,
                FIELD_ARGUMENT_REQUIRED,
                format!(
                    "Required argument `{name}` is missing on `{}.{}`",
                    parent, field.name.value
                ),
                field.name.span,
            );
        }
    }
}

fn fragment_index(
    document: &Document,
) -> BTreeMap<&str, &crate::language::ast::FragmentDefinition> {
    document
        .executable_definitions()
        .filter_map(|definition| match definition {
            ExecutableDefinition::Fragment(fragment) => {
                Some((fragment.name.value.as_str(), fragment))
            }
            ExecutableDefinition::Operation(_) => None,
        })
        .collect()
}

fn push(diagnostics: &mut DiagnosticBag, code: DiagnosticCode, message: String, span: crate::Span) {
    let _ = diagnostics.push(Diagnostic::new(code, message, span));
}

#[cfg(test)]
#[path = "tests_fields.rs"]
mod tests_fields;
