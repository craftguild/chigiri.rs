//! Executable-definition indexing and full-document validation entry point.

use std::collections::BTreeMap;

use crate::language::ast::{Definition, Document, ExecutableDefinition};
use crate::schema::SemanticSchema;
use crate::{Diagnostic, DiagnosticBag, ExecutionLimits};

use super::{
    CustomScalarRegistry, EXECUTABLE_DUPLICATE_FRAGMENT_NAME, EXECUTABLE_DUPLICATE_OPERATION_NAME,
    EXECUTABLE_LONE_ANONYMOUS_OPERATION, EXECUTABLE_TYPE_SYSTEM_DEFINITION,
    ExecutableValidationOutput, ValidatedDocument,
};

pub(super) fn validate(
    schema: &SemanticSchema,
    document: &Document,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> ExecutableValidationOutput {
    let mut diagnostics = DiagnosticBag::new(limits.max_diagnostics);
    let mut had_errors = !super::ast_invariants::validate(document, limits, &mut diagnostics);
    if had_errors {
        return ExecutableValidationOutput {
            document: None,
            diagnostics_truncated: diagnostics.was_truncated() || diagnostics.should_stop(),
            diagnostics,
        };
    }
    let mut operation_indices = Vec::new();
    let mut named_operations = BTreeMap::new();
    let mut fragment_indices = BTreeMap::new();
    let mut anonymous_spans = Vec::new();
    for (index, definition) in document.definitions.iter().enumerate() {
        match definition {
            Definition::TypeSystem(definition) => {
                had_errors = true;
                if !diagnostics.push(Diagnostic::new(
                    EXECUTABLE_TYPE_SYSTEM_DEFINITION,
                    "Type-system definitions cannot appear in an executable document",
                    type_system_span(definition),
                )) || diagnostics.should_stop()
                {
                    break;
                }
            }
            Definition::Executable(ExecutableDefinition::Operation(operation)) => {
                operation_indices.push(index);
                if let Some(name) = &operation.name {
                    if named_operations.insert(name.value.clone(), index).is_some() {
                        had_errors = true;
                        if !diagnostics.push(Diagnostic::new(
                            EXECUTABLE_DUPLICATE_OPERATION_NAME,
                            format!("Operation `{}` is defined more than once", name.value),
                            name.span,
                        )) || diagnostics.should_stop()
                        {
                            break;
                        }
                    }
                } else {
                    anonymous_spans.push(operation.span);
                }
            }
            Definition::Executable(ExecutableDefinition::Fragment(fragment)) => {
                if fragment_indices
                    .insert(fragment.name.value.clone(), index)
                    .is_some()
                {
                    had_errors = true;
                    if !diagnostics.push(Diagnostic::new(
                        EXECUTABLE_DUPLICATE_FRAGMENT_NAME,
                        format!(
                            "Fragment `{}` is defined more than once",
                            fragment.name.value
                        ),
                        fragment.name.span,
                    )) || diagnostics.should_stop()
                    {
                        break;
                    }
                }
            }
        }
    }
    if !diagnostics.should_stop() && operation_indices.len() > 1 {
        for span in anonymous_spans {
            had_errors = true;
            if !diagnostics.push(Diagnostic::new(
                EXECUTABLE_LONE_ANONYMOUS_OPERATION,
                "Anonymous operation must be the only operation in a document",
                span,
            )) || diagnostics.should_stop()
            {
                break;
            }
        }
    }
    if !diagnostics.should_stop()
        && !super::fragment_rules::validate_fragments(schema, document, &mut diagnostics)
    {
        had_errors = true;
    }
    if !diagnostics.should_stop()
        && !super::field_rules::validate_fields(schema, document, &mut diagnostics)
    {
        had_errors = true;
    }
    if !diagnostics.should_stop()
        && !super::variable_rules::validate_variables(schema, document, limits, &mut diagnostics)
    {
        had_errors = true;
    }
    if !diagnostics.should_stop()
        && !super::value_rules::validate_values(schema, document, scalars, limits, &mut diagnostics)
    {
        had_errors = true;
    }
    let (complexities, complexity_valid) = if diagnostics.should_stop() {
        (BTreeMap::new(), true)
    } else {
        super::complexity::summarize(document, limits, &mut diagnostics)
    };
    had_errors |= !complexity_valid;
    let diagnostics_truncated = diagnostics.was_truncated() || diagnostics.should_stop();
    let validated = (!had_errors).then(|| ValidatedDocument {
        complexities,
        document: document.clone(),
        operation_indices,
        named_operations,
        fragment_indices,
    });
    ExecutableValidationOutput {
        document: validated,
        diagnostics,
        diagnostics_truncated,
    }
}

fn type_system_span(definition: &crate::language::ast::TypeSystemDefinition) -> crate::Span {
    use crate::language::ast::TypeSystemDefinition as Definition;
    match definition {
        Definition::Schema(value) => value.span,
        Definition::Scalar(value) => value.span,
        Definition::Object(value) => value.span,
        Definition::Interface(value) => value.span,
        Definition::Union(value) => value.span,
        Definition::Enum(value) => value.span,
        Definition::InputObject(value) => value.span,
        Definition::Directive(value) => value.span,
        Definition::SchemaExtension(value) => value.span,
        Definition::ScalarExtension(value) => value.span,
        Definition::ObjectExtension(value) => value.span,
        Definition::InterfaceExtension(value) => value.span,
        Definition::UnionExtension(value) => value.span,
        Definition::EnumExtension(value) => value.span,
        Definition::InputObjectExtension(value) => value.span,
    }
}
