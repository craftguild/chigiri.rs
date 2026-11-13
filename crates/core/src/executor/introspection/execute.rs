//! Complete introspection fields according to descriptor types and arguments.

use crate::language::ast::{Field, SelectionSet};
use crate::schema::introspection::model::{FieldDescriptor, TypeRefDescriptor, TypeWrapper};
use crate::{FieldError, Location, Object, PathSegment, Value};

use super::model::Node;
use crate::executor::engine::{Completion, Engine, SelectionResult};
use crate::executor::{BoxFuture, CollectedField, collect_fields};

use super::completion::CompletionContext;

impl<C> Engine<'_, C>
where
    C: Send + Sync + 'static,
{
    pub(crate) fn execute_introspection<'a>(
        &'a self,
        group: &'a CollectedField<'a>,
        path: Vec<PathSegment>,
        parent_type: &'a str,
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            let field = group.fields[0];
            let coordinate = (parent_type, field.name.value.as_str());
            let definition = crate::schema::introspection::root_field(&field.name.value)
                .expect("The validated introspection root field has a descriptor.");
            if !self.claim_output_node() {
                return self.output_limit_error(
                    path,
                    self.field_locations(field),
                    outer_non_null(&definition.ty),
                    Some(coordinate),
                    "introspection",
                );
            }
            if !self.introspection_enabled {
                let error = FieldError::public("Schema introspection is disabled")
                    .to_graphql_error(self.field_locations(field), path);
                return Completion {
                    value: Value::Null,
                    errors: vec![error],
                    bubble: outer_non_null(&definition.ty),
                };
            }
            let arguments = match super::arguments::coerce(self, definition, field, &path) {
                Ok(arguments) => arguments,
                Err(errors) => {
                    return Completion {
                        value: Value::Null,
                        errors,
                        bubble: outer_non_null(&definition.ty),
                    };
                }
            };
            let output = super::resolve::root(self.schema, field, &arguments);
            self.complete_introspection(
                output,
                definition.ty,
                0,
                CompletionContext {
                    selection_sets: selections(group),
                    path,
                    locations: self.field_locations(field),
                    coordinate,
                },
            )
            .await
        })
    }

    pub(super) fn execute_node<'a>(
        &'a self,
        node: Node<'a>,
        selection_sets: Vec<&'a SelectionSet>,
        path: Vec<PathSegment>,
    ) -> BoxFuture<'a, SelectionResult> {
        Box::pin(async move {
            let fields = collect_fields(
                self.schema,
                self.document,
                &selection_sets,
                node.runtime_type(),
                self.variables,
            );
            let mut object = Object::with_capacity(fields.len());
            let mut errors = Vec::new();
            let mut bubble = false;
            for group in fields {
                let field = group.fields[0];
                if !self.claim_output_field(&path, group.response_key) {
                    return SelectionResult {
                        value: Object::new(),
                        errors: Vec::new(),
                        bubble: true,
                    };
                }
                let mut field_path = path.clone();
                field_path.push(PathSegment::Field(group.response_key.to_owned()));
                let completion = if field.name.value == "__typename" {
                    if !self.claim_output_bytes(node.runtime_type().len()) {
                        Completion::exhausted()
                    } else if self.claim_output_node() {
                        Completion::value(Value::from(node.runtime_type()))
                    } else {
                        self.output_limit_error(
                            field_path,
                            self.field_locations(field),
                            true,
                            Some((node.runtime_type(), &field.name.value)),
                            "introspection",
                        )
                    }
                } else {
                    self.execute_node_field(node.clone(), &group, field_path)
                        .await
                };
                if self.output_bytes_exhausted() {
                    return SelectionResult {
                        value: Object::new(),
                        errors: Vec::new(),
                        bubble: true,
                    };
                }
                object.insert(group.response_key, completion.value);
                errors.extend(completion.errors);
                bubble |= completion.bubble;
            }
            SelectionResult {
                value: object,
                errors,
                bubble,
            }
        })
    }

    fn execute_node_field<'a>(
        &'a self,
        node: Node<'a>,
        group: &'a CollectedField<'a>,
        path: Vec<PathSegment>,
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            let field = group.fields[0];
            let coordinate = (node.runtime_type(), field.name.value.as_str());
            let definition = introspection_field(node.runtime_type(), &field.name.value)
                .expect("The validated introspection field has a descriptor.");
            if !self.claim_output_node() {
                return self.output_limit_error(
                    path,
                    self.field_locations(field),
                    outer_non_null(&definition.ty),
                    Some(coordinate),
                    "introspection",
                );
            }
            let arguments = match super::arguments::coerce(self, definition, field, &path) {
                Ok(arguments) => arguments,
                Err(errors) => {
                    return Completion {
                        value: Value::Null,
                        errors,
                        bubble: outer_non_null(&definition.ty),
                    };
                }
            };
            let output = super::resolve::node(self.schema, node, field, &arguments);
            self.complete_introspection(
                output,
                definition.ty,
                0,
                CompletionContext {
                    selection_sets: selections(group),
                    path,
                    locations: self.field_locations(field),
                    coordinate,
                },
            )
            .await
        })
    }

    pub(super) fn introspection_error(
        &self,
        message: &str,
        path: Vec<PathSegment>,
        locations: Vec<Location>,
        coordinate: (&str, &str),
    ) -> crate::GraphQlError {
        self.graphql_error(
            FieldError::internal(IntrospectionInvariant(message.to_owned())),
            locations,
            path,
            Some(coordinate),
            "introspection",
        )
    }
}

fn selections<'a>(group: &'a CollectedField<'a>) -> Vec<&'a SelectionSet> {
    group
        .fields
        .iter()
        .filter_map(|field: &&Field| field.selection_set.as_ref())
        .collect()
}

fn introspection_field(parent: &str, name: &str) -> Option<&'static FieldDescriptor> {
    crate::schema::introspection::type_by_name(parent)?
        .fields
        .iter()
        .find(|field| field.name == name)
}

fn outer_non_null(ty: &TypeRefDescriptor) -> bool {
    ty.wrappers.first() == Some(&TypeWrapper::NonNull)
}

#[derive(Debug)]
struct IntrospectionInvariant(String);

impl std::fmt::Display for IntrospectionInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IntrospectionInvariant {}
