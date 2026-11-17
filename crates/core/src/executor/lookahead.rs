//! Read-only effective child selections, using the executor's field collector.
use crate::executable::{CoercedVariables, ValidatedDocument};
use crate::language::ast::{Field, TypeRef, TypeRefKind};
use crate::schema::{SemanticSchema, TypeKind};
use std::fmt;

use super::collect::{collect_fields, type_condition_applies};

/// A selected child field after aliases, fragments and include/skip directives.
///
/// This is selection metadata, not an authorization or execution promise.
/// Collecting it neither invokes resolvers nor expands returned object values.
pub struct SelectedField<'a> {
    environment: Environment<'a>,
    response_key: &'a str,
    name: &'a str,
    output_type: &'a str,
    fields: Vec<&'a Field>,
}

/// Why effective child selection metadata cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// The context was constructed without an active executor selection.
    Unavailable,
    /// The requested concrete object cannot be returned at this position.
    InvalidRuntimeType,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "effective field selection is unavailable",
            Self::InvalidRuntimeType => "selection runtime type is incompatible with the field",
        })
    }
}
impl std::error::Error for SelectionError {}

#[derive(Clone, Copy)]
struct Environment<'a> {
    schema: &'a SemanticSchema,
    document: &'a ValidatedDocument,
    variables: &'a CoercedVariables,
}

#[derive(Clone, Copy)]
pub(super) struct SelectionView<'a> {
    environment: Environment<'a>,
    output_type: &'a str,
    fields: &'a [&'a Field],
}

impl<'a> SelectionView<'a> {
    pub(super) fn new(
        schema: &'a SemanticSchema,
        document: &'a ValidatedDocument,
        variables: &'a CoercedVariables,
        output: &'a TypeRef,
        fields: &'a [&'a Field],
    ) -> Self {
        Self {
            environment: Environment {
                schema,
                document,
                variables,
            },
            output_type: named_type(output),
            fields,
        }
    }

    pub(super) fn selected_fields(
        &self,
        runtime_type: &str,
    ) -> Result<Vec<SelectedField<'a>>, SelectionError> {
        collect(
            self.environment,
            self.output_type,
            self.fields,
            runtime_type,
        )
    }
}

impl<'a> SelectedField<'a> {
    /// The schema field name, independent of any alias.
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// The response key, including any alias.
    pub const fn response_key(&self) -> &'a str {
        self.response_key
    }

    /// The field's named output type, excluding list/non-null wrappers.
    pub const fn output_type(&self) -> &'a str {
        self.output_type
    }

    /// Collect effective children for one compatible concrete runtime object.
    ///
    /// Abstract fields require an explicit concrete type; selections for other
    /// union/interface members are excluded just as during actual execution.
    /// The operation's existing validated expansion bounds apply. Returned
    /// vectors are caller-owned; no response values or background jobs are made.
    pub fn selected_fields(&self, runtime_type: &str) -> Result<Vec<Self>, SelectionError> {
        collect(
            self.environment,
            self.output_type,
            &self.fields,
            runtime_type,
        )
    }
}

fn collect<'a>(
    environment: Environment<'a>,
    output_type: &str,
    fields: &[&'a Field],
    runtime_type: &str,
) -> Result<Vec<SelectedField<'a>>, SelectionError> {
    let schema = environment.schema;
    let object = schema
        .type_by_name(runtime_type)
        .filter(|object| object.kind() == TypeKind::Object)
        .filter(|_| type_condition_applies(schema, output_type, runtime_type))
        .ok_or(SelectionError::InvalidRuntimeType)?;
    let sets: Vec<_> = fields
        .iter()
        .filter_map(|field| field.selection_set.as_ref())
        .collect();
    collect_fields(
        schema,
        environment.document,
        &sets,
        runtime_type,
        environment.variables,
    )
    .into_iter()
    .map(|group| {
        let name = group.fields[0].name.value.as_str();
        let output_type = if name == "__typename" {
            "String"
        } else {
            named_type(
                &object
                    .field(name)
                    .ok_or(SelectionError::InvalidRuntimeType)?
                    .ty,
            )
        };
        Ok(SelectedField {
            environment,
            response_key: group.response_key,
            name,
            output_type,
            fields: group.fields,
        })
    })
    .collect()
}

fn named_type(mut reference: &TypeRef) -> &str {
    loop {
        match &reference.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => reference = inner,
        }
    }
}
