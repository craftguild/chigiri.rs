//! Compose schemas through bounded definition collection, extension application, and validation.

use crate::language::ast::{Definition, Document, ExecutableDefinition, TypeSystemDefinition};
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, ExecutionLimits, Span};

use super::builtins::empty_schema_with_builtins;
use super::{SchemaBuildOutput, SemanticSchema};

pub(super) struct Composer {
    pub(super) schema: SemanticSchema,
    pub(super) diagnostics: DiagnosticBag,
    pub(super) extensions: Vec<TypeSystemDefinition>,
    pub(super) explicit_schema: bool,
    had_errors: bool,
    diagnostic_limit_reached: bool,
}

impl Composer {
    fn new(limits: &ExecutionLimits) -> Self {
        Self {
            schema: empty_schema_with_builtins(),
            diagnostics: DiagnosticBag::new(limits.max_diagnostics),
            extensions: Vec::new(),
            explicit_schema: false,
            had_errors: false,
            diagnostic_limit_reached: false,
        }
    }

    pub(super) fn report(&mut self, code: DiagnosticCode, message: impl Into<String>, span: Span) {
        self.had_errors = true;
        let stored = self.diagnostics.push(Diagnostic::new(code, message, span));
        if !stored || self.diagnostics.is_full() {
            self.diagnostic_limit_reached = true;
        }
    }

    pub(super) fn report_without_span(&mut self, code: DiagnosticCode, message: impl Into<String>) {
        self.had_errors = true;
        let stored = self
            .diagnostics
            .push(Diagnostic::without_span(code, message));
        if !stored || self.diagnostics.is_full() {
            self.diagnostic_limit_reached = true;
        }
    }

    pub(super) fn report_origin(
        &mut self,
        code: DiagnosticCode,
        message: impl Into<String>,
        origin: super::SchemaOrigin,
    ) {
        if let Some(span) = origin.span() {
            self.report(code, message, span);
        } else {
            self.report_without_span(code, message);
        }
    }

    pub(super) fn can_continue(&self) -> bool {
        !self.diagnostic_limit_reached
    }

    pub(super) fn has_errors(&self) -> bool {
        self.had_errors
    }

    fn finish(mut self) -> SchemaBuildOutput {
        if !self.had_errors {
            self.schema.rebuild_possible_object_index();
        }
        SchemaBuildOutput {
            schema: (!self.had_errors).then_some(self.schema),
            diagnostics_truncated: self.diagnostic_limit_reached
                || self.diagnostics.was_truncated(),
            diagnostics: self.diagnostics,
        }
    }
}

pub(super) fn compose(documents: &[Document], limits: &ExecutionLimits) -> SchemaBuildOutput {
    let mut composer = Composer::new(limits);
    super::ast_invariants::validate(&mut composer, documents, limits);
    if composer.has_errors() {
        return composer.finish();
    }
    collect_definitions(&mut composer, documents);
    if composer.can_continue() {
        super::roots::infer_implicit_roots(&mut composer);
    }
    if composer.can_continue() {
        super::extensions::apply_extensions(&mut composer);
    }
    if composer.can_continue() {
        super::source_order::restore(&mut composer.schema, documents);
    }
    if composer.can_continue() {
        super::roots::validate_roots(&mut composer);
    }
    if composer.can_continue() {
        super::directives::validate_directive_applications(&mut composer);
    }
    if composer.can_continue() {
        super::validation::validate_structure(&mut composer);
    }
    composer.finish()
}

fn collect_definitions(composer: &mut Composer, documents: &[Document]) {
    for document in documents {
        for definition in &document.definitions {
            if !composer.can_continue() {
                return;
            }
            match definition {
                Definition::Executable(executable) => composer.report(
                    super::SCHEMA_EXECUTABLE_DEFINITION,
                    "Executable definitions cannot be composed into a schema",
                    executable_span(executable),
                ),
                Definition::TypeSystem(type_system) if is_extension(type_system) => {
                    composer.extensions.push(type_system.clone());
                }
                Definition::TypeSystem(type_system) => {
                    super::definitions::add_definition(composer, type_system)
                }
            }
        }
    }
}

fn executable_span(definition: &ExecutableDefinition) -> Span {
    match definition {
        ExecutableDefinition::Operation(operation) => operation.span,
        ExecutableDefinition::Fragment(fragment) => fragment.span,
    }
}

fn is_extension(definition: &TypeSystemDefinition) -> bool {
    matches!(
        definition,
        TypeSystemDefinition::SchemaExtension(_)
            | TypeSystemDefinition::ScalarExtension(_)
            | TypeSystemDefinition::ObjectExtension(_)
            | TypeSystemDefinition::InterfaceExtension(_)
            | TypeSystemDefinition::UnionExtension(_)
            | TypeSystemDefinition::EnumExtension(_)
            | TypeSystemDefinition::InputObjectExtension(_)
    )
}
