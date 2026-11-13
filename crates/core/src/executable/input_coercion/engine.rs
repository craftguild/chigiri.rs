//! Shared engine that coerces raw values and literals into one presence model.

use crate::language::ast::{TypeRef, TypeRefKind, Value as AstValue, ValueNode};
use crate::schema::{SemanticSchema, TypeKind};
use std::collections::BTreeMap;

use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits, Span, Value};

use super::{
    CoercedInput, CoercedVariables, CustomScalarRegistry, INPUT_CUSTOM_SCALAR_VALUE,
    INPUT_ENUM_VALUE, INPUT_NESTING_LIMIT, INPUT_NULL_FOR_NON_NULL, INPUT_SCALAR_VALUE,
    INPUT_VARIABLE_UNAVAILABLE, InputCoercionOutput,
};

pub(super) enum Source<'value> {
    Raw(Value),
    Literal(&'value ValueNode),
}

pub(super) struct Engine<'context> {
    pub(super) schema: &'context SemanticSchema,
    scalars: &'context CustomScalarRegistry,
    pub(super) variables: Option<&'context CoercedVariables>,
    diagnostics: DiagnosticBag,
    maximum_diagnostics: usize,
    pub(super) maximum_depth: usize,
    fallback_span: Option<Span>,
    path: Vec<String>,
    had_errors: bool,
    pub(super) diagnostic_limit_reached: bool,
    pub(super) budget: Option<&'context super::InputCoercionBudget>,
    pub(super) input_plans: BTreeMap<String, super::plan::InputObjectPlan>,
}

impl<'context> Engine<'context> {
    pub(super) fn new(
        schema: &'context SemanticSchema,
        scalars: &'context CustomScalarRegistry,
        variables: Option<&'context CoercedVariables>,
        limits: &ExecutionLimits,
    ) -> Self {
        Self {
            schema,
            scalars,
            variables,
            diagnostics: DiagnosticBag::new(limits.max_diagnostics),
            maximum_diagnostics: limits.max_diagnostics,
            maximum_depth: limits.max_parser_nesting,
            fallback_span: None,
            path: Vec::new(),
            had_errors: false,
            diagnostic_limit_reached: false,
            budget: None,
            input_plans: BTreeMap::new(),
        }
    }

    pub(super) fn with_budget(
        mut self,
        budget: Option<&'context super::InputCoercionBudget>,
    ) -> Self {
        self.budget = budget;
        self
    }

    pub(super) fn set_fallback_span(&mut self, span: Option<Span>) {
        self.fallback_span = span;
    }

    pub(super) fn push_path(&mut self, segment: String) {
        self.path.push(segment);
    }

    pub(super) fn pop_path(&mut self) {
        self.path.pop();
    }

    pub(super) fn can_continue(&self) -> bool {
        !self.diagnostic_limit_reached
    }

    pub(super) fn coerce(
        &mut self,
        source: Source<'_>,
        expected: &TypeRef,
        depth: usize,
    ) -> Option<CoercedInput> {
        if !self.can_continue() {
            return None;
        }
        if depth > self.maximum_depth {
            let span = source_span(&source).or(self.fallback_span);
            self.report(
                INPUT_NESTING_LIMIT,
                "Input value nesting limit exceeded",
                span,
            );
            return None;
        }
        if let Source::Literal(node) = &source
            && let AstValue::Variable(name) = &node.value
        {
            return self.variable(name.value.as_str(), name.span, expected);
        }
        if let TypeRefKind::NonNull(inner) = &expected.kind {
            if source_is_null(&source) {
                self.report(
                    INPUT_NULL_FOR_NON_NULL,
                    "Non-null input type cannot accept null",
                    source_span(&source).or(self.fallback_span),
                );
                return None;
            }
            return self.coerce(source, inner, depth.saturating_add(1));
        }
        if !self.reserve(
            1,
            super::budget::source_bytes(&source),
            source_span(&source),
        ) {
            if let Source::Raw(value) = source {
                crate::value::drop_iterative(value);
            }
            return None;
        }
        if source_is_null(&source) {
            return Some(CoercedInput::Null);
        }
        match &expected.kind {
            TypeRefKind::List(item) => self.coerce_list(source, item, depth.saturating_add(1)),
            TypeRefKind::Named(name) => self.coerce_named(source, &name.value, depth),
            TypeRefKind::NonNull(_) => unreachable!("non-null is handled first"),
        }
    }

    fn variable(&mut self, name: &str, span: Span, expected: &TypeRef) -> Option<CoercedInput> {
        let Some(value) = self.variables.and_then(|variables| variables.get(name)) else {
            self.report(
                INPUT_VARIABLE_UNAVAILABLE,
                format!("Variable `${name}` has not been coerced"),
                Some(span),
            );
            return None;
        };
        match value {
            CoercedInput::Undefined => Some(CoercedInput::Undefined),
            CoercedInput::Null if matches!(expected.kind, TypeRefKind::NonNull(_)) => {
                self.report(
                    INPUT_NULL_FOR_NON_NULL,
                    format!("Variable `${name}` cannot be null at a non-null position"),
                    Some(span),
                );
                None
            }
            CoercedInput::Null => self.reserve(1, 0, Some(span)).then_some(CoercedInput::Null),
            CoercedInput::Value(value) => {
                if !self.reserve_value(value, Some(span)) {
                    return None;
                }
                Some(CoercedInput::Value(value.clone()))
            }
        }
    }

    fn coerce_list(
        &mut self,
        source: Source<'_>,
        item_type: &TypeRef,
        depth: usize,
    ) -> Option<CoercedInput> {
        let sources = match source {
            Source::Raw(Value::List(items)) => items.into_iter().map(Source::Raw).collect(),
            Source::Literal(node) => match &node.value {
                AstValue::List(items) => items.iter().map(Source::Literal).collect(),
                _ => vec![Source::Literal(node)],
            },
            other => vec![other],
        };
        let mut values = Vec::with_capacity(sources.len());
        let mut valid = true;
        for (index, source) in sources.into_iter().enumerate() {
            self.push_path(format!("[{index}]"));
            match self.coerce(source, item_type, depth) {
                Some(CoercedInput::Value(value)) => values.push(value),
                Some(CoercedInput::Null) => values.push(Value::Null),
                Some(CoercedInput::Undefined)
                    if !matches!(item_type.kind, TypeRefKind::NonNull(_)) =>
                {
                    if !self.reserve(1, 0, self.fallback_span) {
                        return None;
                    }
                    values.push(Value::Null);
                }
                Some(CoercedInput::Undefined) => {
                    valid = false;
                    self.report(
                        INPUT_NULL_FOR_NON_NULL,
                        "A non-null list item cannot be undefined",
                        self.fallback_span,
                    );
                }
                None => valid = false,
            }
            self.pop_path();
            if !self.can_continue() {
                valid = false;
                break;
            }
        }
        valid.then_some(CoercedInput::Value(Value::List(values)))
    }

    fn coerce_named(
        &mut self,
        source: Source<'_>,
        type_name: &str,
        depth: usize,
    ) -> Option<CoercedInput> {
        let Some(named) = self.schema.type_by_name(type_name) else {
            self.report(
                INPUT_SCALAR_VALUE,
                format!("Input type `{type_name}` is not defined"),
                source_span(&source).or(self.fallback_span),
            );
            return None;
        };
        match named.kind() {
            TypeKind::Scalar => self.coerce_scalar(source, type_name, depth),
            TypeKind::Enum => {
                let span = source_span(&source).or(self.fallback_span);
                let candidate = match source {
                    Source::Raw(Value::String(value)) => Some(value),
                    Source::Literal(node) => match &node.value {
                        AstValue::Enum(name) => Some(name.value.clone()),
                        _ => None,
                    },
                    Source::Raw(_) => None,
                };
                match candidate.filter(|value| named.enum_value(value).is_some()) {
                    Some(value) => Some(CoercedInput::Value(Value::String(value))),
                    None => {
                        self.report(
                            INPUT_ENUM_VALUE,
                            format!("Input is not a defined member of enum `{type_name}`"),
                            span,
                        );
                        None
                    }
                }
            }
            TypeKind::InputObject => super::objects::coerce(self, named, source, depth),
            TypeKind::Object | TypeKind::Interface | TypeKind::Union => {
                self.report(
                    INPUT_SCALAR_VALUE,
                    format!("Type `{type_name}` cannot be used as input"),
                    source_span(&source).or(self.fallback_span),
                );
                None
            }
        }
    }

    fn coerce_scalar(
        &mut self,
        source: Source<'_>,
        name: &str,
        depth: usize,
    ) -> Option<CoercedInput> {
        let span = source_span(&source).or(self.fallback_span);
        let value = match name {
            "Int" => super::builtin::int(source),
            "Float" => super::builtin::float(source),
            "String" => super::builtin::string(source),
            "Boolean" => super::builtin::boolean(source),
            "ID" => super::builtin::id(source),
            _ => {
                let value = super::runtime_value::convert(self, source, depth)?;
                return match self.scalars.coerce(name, value) {
                    Ok(Value::Null) => {
                        self.report(
                            INPUT_CUSTOM_SCALAR_VALUE,
                            format!("Custom scalar `{name}` coercer returned null"),
                            span,
                        );
                        None
                    }
                    Ok(value) => Some(CoercedInput::Value(value)),
                    Err(error) => {
                        self.report(INPUT_CUSTOM_SCALAR_VALUE, error.message(), span);
                        None
                    }
                };
            }
        };
        match value {
            Some(value) => Some(CoercedInput::Value(value)),
            None => {
                self.report(
                    INPUT_SCALAR_VALUE,
                    format!("Input is incompatible with scalar `{name}`"),
                    span,
                );
                None
            }
        }
    }

    pub(super) fn report(
        &mut self,
        code: DiagnosticCode,
        message: impl AsRef<str>,
        span: Option<Span>,
    ) {
        self.had_errors = true;
        if self.diagnostic_limit_reached {
            return;
        }
        let (diagnostic, exhausted) = super::diagnostics::build(
            code,
            message.as_ref(),
            span.or(self.fallback_span),
            &self.path,
            self.budget,
        );
        if exhausted {
            self.diagnostics = DiagnosticBag::new(self.maximum_diagnostics.min(1));
            self.diagnostic_limit_reached = true;
        }
        let stored = self.diagnostics.push(diagnostic);
        if !stored || self.diagnostics.is_full() {
            self.diagnostic_limit_reached = true;
        }
    }

    pub(super) fn finish(self, value: Option<CoercedInput>) -> InputCoercionOutput {
        let valid = !self.had_errors;
        let diagnostics_truncated =
            self.diagnostic_limit_reached || self.diagnostics.was_truncated();
        InputCoercionOutput {
            value: valid.then_some(value).flatten(),
            diagnostics: self.diagnostics,
            diagnostics_truncated,
        }
    }
}

fn source_span(source: &Source<'_>) -> Option<Span> {
    match source {
        Source::Literal(node) => Some(node.span),
        Source::Raw(_) => None,
    }
}

fn source_is_null(source: &Source<'_>) -> bool {
    matches!(source, Source::Raw(Value::Null))
        || matches!(source, Source::Literal(node) if matches!(node.value, AstValue::Null))
}
