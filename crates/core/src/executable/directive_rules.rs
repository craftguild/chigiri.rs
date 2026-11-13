//! Validate executable-directive definitions, locations, repeatability, and arguments.

use std::collections::BTreeSet;

use crate::language::ast::{Directive, DirectiveLocation};
use crate::schema::SemanticSchema;
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode};

pub(super) const DIRECTIVE_UNKNOWN: DiagnosticCode = DiagnosticCode::new("SF3040");
pub(super) const DIRECTIVE_LOCATION_INVALID: DiagnosticCode = DiagnosticCode::new("SF3041");
pub(super) const DIRECTIVE_DUPLICATE: DiagnosticCode = DiagnosticCode::new("SF3042");
pub(super) const DIRECTIVE_ARGUMENT_DUPLICATE: DiagnosticCode = DiagnosticCode::new("SF3043");
pub(super) const DIRECTIVE_ARGUMENT_UNKNOWN: DiagnosticCode = DiagnosticCode::new("SF3044");
pub(super) const DIRECTIVE_ARGUMENT_REQUIRED: DiagnosticCode = DiagnosticCode::new("SF3045");

pub(super) fn validate_directives(
    schema: &SemanticSchema,
    applications: &[Directive],
    location: DirectiveLocation,
    diagnostics: &mut DiagnosticBag,
) {
    let mut seen = BTreeSet::new();
    for application in applications {
        if diagnostics.should_stop() {
            return;
        }
        let Some(rule) = schema.directive_definition(&application.name.value) else {
            push(
                diagnostics,
                DIRECTIVE_UNKNOWN,
                format!("Directive `@{}` is not defined", application.name.value),
                application.name.span,
            );
            continue;
        };
        if !rule.allows_location(location) {
            push(
                diagnostics,
                DIRECTIVE_LOCATION_INVALID,
                format!(
                    "Directive `@{}` is not allowed at {:?}",
                    application.name.value, location
                ),
                application.name.span,
            );
        }
        if diagnostics.should_stop() {
            return;
        }
        if !rule.is_repeatable() && !seen.insert(application.name.value.as_str()) {
            push(
                diagnostics,
                DIRECTIVE_DUPLICATE,
                format!(
                    "Directive `@{}` is not repeatable at this location",
                    application.name.value
                ),
                application.name.span,
            );
        }
        if diagnostics.should_stop() {
            return;
        }
        validate_application(rule, application, diagnostics);
    }
}

fn validate_application(
    rule: &crate::schema::SemanticDirectiveDefinition,
    application: &Directive,
    diagnostics: &mut DiagnosticBag,
) {
    let mut supplied = BTreeSet::new();
    for argument in &application.arguments {
        if diagnostics.should_stop() {
            return;
        }
        if !supplied.insert(argument.name.value.as_str()) {
            push(
                diagnostics,
                DIRECTIVE_ARGUMENT_DUPLICATE,
                format!(
                    "Directive argument `{}` is supplied more than once",
                    argument.name.value
                ),
                argument.name.span,
            );
        }
        if diagnostics.should_stop() {
            return;
        }
        if rule.argument(&argument.name.value).is_none() {
            push(
                diagnostics,
                DIRECTIVE_ARGUMENT_UNKNOWN,
                format!(
                    "Argument `{}` is not defined by `@{}`",
                    argument.name.value, application.name.value
                ),
                argument.name.span,
            );
        }
    }
    for definition in rule.required_arguments() {
        let name = definition.name.as_str();
        if diagnostics.should_stop() {
            return;
        }
        if !supplied.contains(name) {
            push(
                diagnostics,
                DIRECTIVE_ARGUMENT_REQUIRED,
                format!(
                    "Required argument `{name}` is missing from `@{}`",
                    application.name.value
                ),
                application.name.span,
            );
        }
    }
}

fn push(diagnostics: &mut DiagnosticBag, code: DiagnosticCode, message: String, span: crate::Span) {
    let _ = diagnostics.push(Diagnostic::new(code, message, span));
}
