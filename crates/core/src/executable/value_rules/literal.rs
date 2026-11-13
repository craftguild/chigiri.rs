//! Recheck literal shapes and record the expected type at each variable span.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{Name, TypeRef, Value as AstValue, ValueNode};
use crate::schema::{SemanticSchema, TypeKind};
use crate::{Diagnostic, DiagnosticBag, DiagnosticCode, ExecutionLimits, Span};

use super::required::RequiredFieldCache;
use super::types::InputType;
use super::{
    CustomScalarRegistry, INPUT_CUSTOM_SCALAR_VALUE, INPUT_DUPLICATE_FIELD, INPUT_ENUM_VALUE,
    INPUT_NULL_FOR_NON_NULL, INPUT_OBJECT_VALUE, INPUT_ONE_OF_VALUE, INPUT_REQUIRED_VALUE_MISSING,
    INPUT_SCALAR_VALUE, INPUT_UNKNOWN_FIELD,
};

pub(super) type SiteKey = usize;
pub(super) type SiteMap<'a> = BTreeMap<SiteKey, VariableSite<'a>>;

#[derive(Clone)]
pub(super) struct VariableSite<'a> {
    pub(super) expected: InputType<'a>,
    pub(super) location_default: bool,
    pub(super) one_of_field: bool,
}

struct Job<'value, 'a> {
    value: &'value ValueNode,
    expected: InputType<'a>,
    location_default: bool,
    one_of_field: bool,
}

pub(super) struct Inspector<'a, 'run> {
    schema: &'a SemanticSchema,
    scalars: &'run CustomScalarRegistry,
    limits: &'run ExecutionLimits,
    diagnostics: &'run mut DiagnosticBag,
    sites: SiteMap<'a>,
    required_fields: RequiredFieldCache<'a>,
    valid: bool,
    stopped: bool,
}

impl<'a, 'run> Inspector<'a, 'run> {
    pub(super) fn new(
        schema: &'a SemanticSchema,
        scalars: &'run CustomScalarRegistry,
        limits: &'run ExecutionLimits,
        diagnostics: &'run mut DiagnosticBag,
    ) -> Self {
        Self {
            schema,
            scalars,
            limits,
            diagnostics,
            sites: BTreeMap::new(),
            required_fields: RequiredFieldCache::default(),
            valid: true,
            stopped: false,
        }
    }

    pub(super) fn inspect(
        &mut self,
        value: &ValueNode,
        expected: InputType<'a>,
        location_default: bool,
    ) -> bool {
        let mut work = vec![Job {
            value,
            expected,
            location_default,
            one_of_field: false,
        }];
        while let Some(mut job) = work.pop() {
            if self.stopped {
                return false;
            }
            if let AstValue::Variable(name) = &job.value.value {
                self.sites.insert(
                    name_key(name),
                    VariableSite {
                        expected: job.expected,
                        location_default: job.location_default,
                        one_of_field: job.one_of_field,
                    },
                );
                continue;
            }
            if job.expected.is_non_null() {
                if matches!(job.value.value, AstValue::Null) {
                    self.report(
                        INPUT_NULL_FOR_NON_NULL,
                        "Non-null input type cannot accept null",
                        job.value.span,
                    );
                    continue;
                }
                job.expected = job.expected.peel();
            } else if matches!(job.value.value, AstValue::Null) {
                continue;
            }
            if job.expected.is_list() {
                let item = job.expected.peel();
                match &job.value.value {
                    AstValue::List(items) => work.extend(items.iter().rev().map(|value| Job {
                        value,
                        expected: item.clone(),
                        location_default: false,
                        one_of_field: false,
                    })),
                    _ => work.push(Job {
                        value: job.value,
                        expected: item,
                        location_default: false,
                        one_of_field: false,
                    }),
                }
                continue;
            }
            let Some(named) = self.schema.type_by_name(job.expected.named_type()) else {
                continue;
            };
            match named.kind() {
                TypeKind::Scalar if super::builtin::is_builtin(named.name()) => {
                    if !super::builtin::valid(named.name(), &job.value.value) {
                        self.report(
                            INPUT_SCALAR_VALUE,
                            format!("Input is incompatible with scalar `{}`", named.name()),
                            job.value.span,
                        );
                    }
                }
                TypeKind::Scalar => {
                    self.custom_scalar(named.name(), job.value, &job.expected, job.location_default)
                }
                TypeKind::Enum => {
                    let valid = match &job.value.value {
                        AstValue::Enum(value) => named.enum_value(&value.value).is_some(),
                        _ => false,
                    };
                    if !valid {
                        self.report(
                            INPUT_ENUM_VALUE,
                            format!("Input is not a defined member of enum `{}`", named.name()),
                            job.value.span,
                        );
                    }
                }
                TypeKind::InputObject => self.input_object(named, job.value, &mut work),
                TypeKind::Object | TypeKind::Interface | TypeKind::Union => {}
            }
        }
        !self.stopped
    }

    pub(super) fn finish(self) -> (bool, SiteMap<'a>) {
        (self.valid && !self.stopped, self.sites)
    }

    fn input_object<'value>(
        &mut self,
        named: &'a crate::schema::SemanticType,
        value: &'value ValueNode,
        work: &mut Vec<Job<'value, 'a>>,
    ) {
        let AstValue::Object(fields) = &value.value else {
            self.report(
                INPUT_OBJECT_VALUE,
                format!("Expected an input object value for `{}`", named.name()),
                value.span,
            );
            return;
        };
        let one_of = named
            .directives()
            .iter()
            .any(|directive| directive.name.value == "oneOf");
        if one_of && (fields.len() != 1 || matches!(fields[0].value.value, AstValue::Null)) {
            self.report(
                INPUT_ONE_OF_VALUE,
                format!(
                    "OneOf input object `{}` requires exactly one non-null field",
                    named.name()
                ),
                value.span,
            );
            if self.stopped {
                return;
            }
        }
        let mut seen = BTreeSet::new();
        let mut supplied = BTreeSet::new();
        let mut children = Vec::new();
        for field in fields {
            #[cfg(test)]
            super::metrics::record_input_field();
            if !seen.insert(field.name.value.as_str()) {
                self.report(
                    INPUT_DUPLICATE_FIELD,
                    format!(
                        "Input field `{}` is provided more than once",
                        field.name.value
                    ),
                    field.name.span,
                );
                if self.stopped {
                    return;
                }
            }
            let Some(definition) = named.input_field(&field.name.value) else {
                self.report(
                    INPUT_UNKNOWN_FIELD,
                    format!(
                        "Input object `{}` has no field `{}`",
                        named.name(),
                        field.name.value
                    ),
                    field.name.span,
                );
                if self.stopped {
                    return;
                }
                continue;
            };
            supplied.insert(field.name.value.as_str());
            children.push(Job {
                value: &field.value,
                expected: InputType::from_ast(&definition.ty),
                location_default: definition.default_value.is_some(),
                one_of_field: one_of && fields.len() == 1,
            });
        }
        for index in self.required_fields.indices(named) {
            let definition = &named.input_fields()[index];
            if !supplied.contains(definition.name.value.as_str()) {
                self.report(
                    INPUT_REQUIRED_VALUE_MISSING,
                    format!(
                        "Input object `{}` requires field `{}`",
                        named.name(),
                        definition.name.value
                    ),
                    value.span,
                );
                if self.stopped {
                    return;
                }
            }
        }
        work.extend(children.into_iter().rev());
    }

    fn custom_scalar(
        &mut self,
        scalar_name: &str,
        root: &ValueNode,
        expected: &InputType<'a>,
        location_default: bool,
    ) {
        let scan = super::custom::scan(root, |name, span| {
            self.report(
                INPUT_DUPLICATE_FIELD,
                format!("Input field `{name}` is provided more than once"),
                span,
            );
            !self.stopped
        });
        if self.stopped {
            return;
        }
        for name in scan.variables.iter().copied() {
            self.sites.insert(
                name_key(name),
                VariableSite {
                    expected: expected.clone(),
                    location_default,
                    one_of_field: false,
                },
            );
        }
        if !scan.variables.is_empty() || scan.has_duplicates {
            return;
        }
        let raw = match super::custom::normalize(root) {
            Ok(value) => value,
            Err(span) => {
                self.report(
                    INPUT_SCALAR_VALUE,
                    "Numeric literal cannot be represented as a core value",
                    span,
                );
                return;
            }
        };
        let reference = TypeRef::named(Name::new(scalar_name.to_owned(), root.span), root.span);
        let output =
            super::super::coerce_raw_input(self.schema, &reference, raw, self.scalars, self.limits);
        if output.is_valid() {
            return;
        }
        let mut reported = false;
        for diagnostic in output.diagnostics {
            reported = true;
            self.report(
                diagnostic.code,
                diagnostic.message,
                diagnostic.span.unwrap_or(root.span),
            );
            if self.stopped {
                return;
            }
        }
        if !reported {
            self.report(
                INPUT_CUSTOM_SCALAR_VALUE,
                format!("Custom scalar `{scalar_name}` rejected the literal"),
                root.span,
            );
        }
    }

    fn report(&mut self, code: DiagnosticCode, message: impl Into<String>, span: Span) {
        self.valid = false;
        if !self.diagnostics.push(Diagnostic::new(code, message, span))
            || self.diagnostics.should_stop()
        {
            self.stopped = true;
        }
    }
}

/// Returns an AST node identity valid only while processing the same document.
pub(super) fn name_key(name: &Name) -> SiteKey {
    std::ptr::from_ref(name) as usize
}
