//! Collects validation jobs for defaults and type-system directive applications.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{Directive, Value, ValueNode};
use crate::schema::{SemanticSchema, SemanticTypeRef, SemanticTypeRefKind};

use super::{ExpectedType, Job};

pub(super) fn collect(schema: &SemanticSchema) -> Vec<Job> {
    let mut jobs = Vec::new();
    collect_defaults(schema, &mut jobs);
    let arguments = directive_argument_types(schema);
    collect_directive_applications(schema, &arguments, &mut jobs);
    jobs
}

fn collect_defaults(schema: &SemanticSchema, jobs: &mut Vec<Job>) {
    for named_type in &schema.types {
        for field in &named_type.fields {
            for argument in &field.arguments {
                push_source_default(argument, jobs);
            }
        }
        for field in &named_type.input_fields {
            push_source_default(field, jobs);
        }
    }
    for definition in &schema.directives {
        for argument in definition.arguments() {
            let Some(source) = &argument.source_definition else {
                continue;
            };
            let Some(value) = &source.default_value else {
                continue;
            };
            jobs.push(Job {
                expected: ExpectedType::from_semantic(&argument.ty),
                value: value.clone(),
            });
        }
    }
}

fn push_source_default(input: &crate::language::ast::InputValueDefinition, jobs: &mut Vec<Job>) {
    if let Some(value) = &input.default_value {
        jobs.push(Job {
            expected: ExpectedType::from_source(&input.ty),
            value: value.clone(),
        });
    }
}

fn directive_argument_types(
    schema: &SemanticSchema,
) -> BTreeMap<String, BTreeMap<String, ExpectedType>> {
    schema
        .directives
        .iter()
        .map(|definition| {
            let arguments = definition
                .arguments()
                .iter()
                .map(|argument| {
                    (
                        argument.name.clone(),
                        ExpectedType::from_semantic(&argument.ty),
                    )
                })
                .collect();
            (definition.name().to_owned(), arguments)
        })
        .collect()
}

fn collect_directive_applications(
    schema: &SemanticSchema,
    definitions: &BTreeMap<String, BTreeMap<String, ExpectedType>>,
    jobs: &mut Vec<Job>,
) {
    push_applications(&schema.schema_directives, definitions, jobs);
    for named_type in &schema.types {
        push_applications(&named_type.directives, definitions, jobs);
        for field in &named_type.fields {
            push_applications(&field.directives, definitions, jobs);
            for argument in &field.arguments {
                push_applications(&argument.directives, definitions, jobs);
            }
        }
        for value in &named_type.enum_values {
            push_applications(&value.directives, definitions, jobs);
        }
        for field in &named_type.input_fields {
            push_applications(&field.directives, definitions, jobs);
        }
    }
    for definition in &schema.directives {
        for argument in definition.arguments() {
            if let Some(source) = &argument.source_definition {
                push_applications(&source.directives, definitions, jobs);
            }
        }
    }
}

fn push_applications(
    applications: &[Directive],
    definitions: &BTreeMap<String, BTreeMap<String, ExpectedType>>,
    jobs: &mut Vec<Job>,
) {
    for application in applications {
        let Some(arguments) = definitions.get(&application.name.value) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for provided in &application.arguments {
            if !seen.insert(provided.name.value.as_str()) || contains_variable(&provided.value) {
                continue;
            }
            let Some(expected) = arguments.get(&provided.name.value) else {
                continue;
            };
            jobs.push(Job {
                expected: expected.clone(),
                value: provided.value.clone(),
            });
        }
    }
}

// variable is the only time that the preceding directive structure pass is diagnosed with SF2045.
fn contains_variable(value: &ValueNode) -> bool {
    let mut work = vec![value];
    while let Some(current) = work.pop() {
        match &current.value {
            Value::Variable(_) => return true,
            Value::List(items) => work.extend(items),
            Value::Object(fields) => work.extend(fields.iter().map(|field| &field.value)),
            Value::Int(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Boolean(_)
            | Value::Null
            | Value::Enum(_) => {}
        }
    }
    false
}

impl ExpectedType {
    fn from_semantic(reference: &SemanticTypeRef) -> Self {
        match &reference.kind {
            SemanticTypeRefKind::Named(name) => Self::Named(name.as_str().into()),
            SemanticTypeRefKind::List(item) => Self::List(Box::new(Self::from_semantic(item))),
            SemanticTypeRefKind::NonNull(inner) => {
                Self::NonNull(Box::new(Self::from_semantic(inner)))
            }
        }
    }
}
