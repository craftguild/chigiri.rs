//! Recheck public executable-AST invariants before validation.

use crate::language::ast::{
    Definition, Directive, Document, ExecutableDefinition, Name, Selection, SelectionSet, TypeRef,
    TypeRefKind, Value as AstValue, ValueNode,
};
use crate::language::numeric::{valid_float, valid_int};
use crate::{Diagnostic, DiagnosticBag, ExecutionLimits, Span};

use super::{EXECUTABLE_AST_NESTING_LIMIT, EXECUTABLE_INVALID_AST_SYNTAX};

pub(super) fn validate(
    document: &Document,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let mut validator = Validator {
        diagnostics,
        invalid: false,
        max_selection_nesting: limits.max_parser_nesting,
        max_value_nesting: limits.max_parser_nesting,
        max_type_layers: limits
            .max_parser_nesting
            .saturating_mul(2)
            .saturating_add(1),
    };
    if document.definitions.is_empty() {
        validator.invalid_without_span("Executable document must contain a definition");
    }
    for definition in &document.definitions {
        if !validator.can_continue() {
            break;
        }
        if let Definition::Executable(definition) = definition {
            validator.definition(definition);
        }
    }
    !validator.invalid
}

struct Validator<'a> {
    diagnostics: &'a mut DiagnosticBag,
    invalid: bool,
    max_selection_nesting: usize,
    max_value_nesting: usize,
    max_type_layers: usize,
}

impl Validator<'_> {
    fn definition(&mut self, definition: &ExecutableDefinition) {
        match definition {
            ExecutableDefinition::Operation(operation) => {
                if let Some(name) = &operation.name {
                    self.name(name, "operation");
                }
                if operation.is_shorthand()
                    && (operation.description.is_some()
                        || operation.operation_type != crate::language::ast::OperationType::Query
                        || operation.name.is_some()
                        || !operation.variable_definitions.is_empty()
                        || !operation.directives.is_empty())
                {
                    self.invalid(
                        "Shorthand operation cannot have a description, name, variables, or directives",
                        operation.span,
                    );
                }
                for variable in &operation.variable_definitions {
                    self.name(&variable.variable, "variable definition");
                    self.type_ref(&variable.ty);
                    if let Some(value) = &variable.default_value {
                        self.value(value, false);
                    }
                    self.directives(&variable.directives, false);
                }
                self.directives(&operation.directives, true);
                self.selection_set(&operation.selection_set);
            }
            ExecutableDefinition::Fragment(fragment) => {
                self.name(&fragment.name, "fragment");
                if fragment.name.value == "on" {
                    self.invalid("Fragment name cannot be `on`", fragment.name.span);
                }
                self.name(&fragment.type_condition, "fragment type condition");
                self.directives(&fragment.directives, true);
                self.selection_set(&fragment.selection_set);
            }
        }
    }

    fn selection_set(&mut self, root: &SelectionSet) {
        let mut work = vec![(root, 1_usize)];
        while let Some((set, depth)) = work.pop() {
            if set.selections.is_empty() {
                self.invalid("Selection set must not be empty", set.span);
            }
            for selection in &set.selections {
                match selection {
                    Selection::Field(field) => {
                        if let Some(alias) = &field.alias {
                            self.name(alias, "field alias");
                        }
                        self.name(&field.name, "field");
                        for argument in &field.arguments {
                            self.name(&argument.name, "field argument");
                            self.value(&argument.value, true);
                        }
                        self.directives(&field.directives, true);
                        if let Some(nested) = &field.selection_set {
                            self.push_selection(&mut work, nested, depth);
                        }
                    }
                    Selection::FragmentSpread(spread) => {
                        self.name(&spread.name, "fragment spread");
                        if spread.name.value == "on" {
                            self.invalid("Fragment spread name cannot be `on`", spread.name.span);
                        }
                        self.directives(&spread.directives, true);
                    }
                    Selection::InlineFragment(fragment) => {
                        if let Some(condition) = &fragment.type_condition {
                            self.name(condition, "inline fragment type condition");
                        }
                        self.directives(&fragment.directives, true);
                        self.push_selection(&mut work, &fragment.selection_set, depth);
                    }
                }
                if !self.can_continue() {
                    return;
                }
            }
        }
    }

    fn push_selection<'a>(
        &mut self,
        work: &mut Vec<(&'a SelectionSet, usize)>,
        nested: &'a SelectionSet,
        depth: usize,
    ) {
        if depth >= self.max_selection_nesting {
            self.nesting("Selection set", nested.span);
        } else {
            work.push((nested, depth + 1));
        }
    }

    fn directives(&mut self, directives: &[Directive], variables_allowed: bool) {
        for directive in directives {
            self.name(&directive.name, "directive application");
            for argument in &directive.arguments {
                self.name(&argument.name, "directive argument");
                self.value(&argument.value, variables_allowed);
            }
            if !self.can_continue() {
                return;
            }
        }
    }

    fn value(&mut self, root: &ValueNode, variables_allowed: bool) {
        let mut work = vec![(root, 0_usize)];
        while let Some((value, depth)) = work.pop() {
            match &value.value {
                AstValue::Variable(name) => {
                    self.name(name, "variable value");
                    if !variables_allowed {
                        self.invalid("Constant value cannot contain a variable", value.span);
                    }
                }
                AstValue::Int(raw) if !valid_int(raw) => {
                    self.invalid("Invalid GraphQL IntValue lexeme", value.span);
                }
                AstValue::Float(raw) if !valid_float(raw) => {
                    self.invalid("Invalid GraphQL FloatValue lexeme", value.span);
                }
                AstValue::Enum(name) => {
                    self.name(name, "enum value literal");
                    if matches!(name.value.as_str(), "true" | "false" | "null") {
                        self.invalid("Literal uses an invalid AST value variant", name.span);
                    }
                }
                AstValue::List(items) => {
                    self.push_values(&mut work, items.iter(), depth, value.span);
                }
                AstValue::Object(fields) => {
                    for field in fields {
                        self.name(&field.name, "input object field");
                    }
                    self.push_values(
                        &mut work,
                        fields.iter().map(|field| &field.value),
                        depth,
                        value.span,
                    );
                }
                AstValue::Int(_)
                | AstValue::Float(_)
                | AstValue::String(_)
                | AstValue::Boolean(_)
                | AstValue::Null => {}
            }
            if !self.can_continue() {
                return;
            }
        }
    }

    fn push_values<'a>(
        &mut self,
        work: &mut Vec<(&'a ValueNode, usize)>,
        values: impl DoubleEndedIterator<Item = &'a ValueNode>,
        depth: usize,
        span: Span,
    ) {
        if depth >= self.max_value_nesting {
            self.nesting("Value", span);
        } else {
            work.extend(values.rev().map(|value| (value, depth + 1)));
        }
    }

    fn type_ref(&mut self, reference: &TypeRef) {
        let mut current = reference;
        let mut layers = 1_usize;
        loop {
            if layers > self.max_type_layers {
                self.nesting("Type reference", current.span);
                return;
            }
            match &current.kind {
                TypeRefKind::Named(name) => {
                    self.name(name, "type reference");
                    return;
                }
                TypeRefKind::List(inner) => current = inner,
                TypeRefKind::NonNull(inner) => {
                    if matches!(inner.kind, TypeRefKind::NonNull(_)) {
                        self.invalid(
                            "Non-null type cannot wrap another non-null type",
                            current.span,
                        );
                        return;
                    }
                    current = inner;
                }
            }
            layers += 1;
        }
    }

    fn name(&mut self, name: &Name, element: &str) {
        if !valid_name(&name.value) {
            self.invalid(
                format!("Invalid GraphQL Name `{}` for {element}", name.value),
                name.span,
            );
        }
    }

    fn invalid(&mut self, message: impl Into<String>, span: Span) {
        self.invalid = true;
        let _ = self.diagnostics.push(Diagnostic::new(
            EXECUTABLE_INVALID_AST_SYNTAX,
            message,
            span,
        ));
    }

    fn invalid_without_span(&mut self, message: impl Into<String>) {
        self.invalid = true;
        let _ = self.diagnostics.push(Diagnostic::without_span(
            EXECUTABLE_INVALID_AST_SYNTAX,
            message,
        ));
    }

    fn nesting(&mut self, element: &str, span: Span) {
        self.invalid = true;
        let _ = self.diagnostics.push(Diagnostic::new(
            EXECUTABLE_AST_NESTING_LIMIT,
            format!("{element} nesting exceeds the configured parser limit"),
            span,
        ));
    }

    fn can_continue(&self) -> bool {
        !self.diagnostics.should_stop()
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
