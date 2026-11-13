//! Execute collected fields concurrently or serialize mutation-root fields.

use crate::language::ast::{Field, SelectionSet};
use crate::schema::TypeKind;
use crate::{Object, PathSegment, Value};

use super::engine::{Completion, Engine, SelectionResult};
use super::{
    BoxFuture, FieldContext, FieldNext, ResolvedObject, SelectionMetadata, collect_fields,
};

impl<C> Engine<'_, C>
where
    C: Send + Sync + 'static,
{
    pub(super) fn execute_selection<'a>(
        &'a self,
        selection_sets: Vec<&'a SelectionSet>,
        declared_type: &'a str,
        runtime_type: &'a str,
        parent: Option<&'a ResolvedObject>,
        path: Vec<PathSegment>,
        serial: bool,
    ) -> BoxFuture<'a, SelectionResult> {
        Box::pin(async move {
            let fields = collect_fields(
                self.schema,
                self.document,
                &selection_sets,
                runtime_type,
                self.variables,
            );
            let mut values = Object::with_capacity(fields.len());
            let mut errors = Vec::new();
            if serial {
                for field in &fields {
                    let completion = self
                        .execute_field(field, declared_type, runtime_type, parent, &path)
                        .await;
                    values.insert(field.response_key, completion.value);
                    errors.extend(completion.errors);
                    if completion.bubble {
                        return SelectionResult {
                            value: Object::new(),
                            errors,
                            bubble: true,
                        };
                    }
                }
            } else {
                let completions = self
                    .execute_normal(&fields, declared_type, runtime_type, parent, &path)
                    .await;
                let mut bubble = false;
                for (field, completion) in fields.iter().zip(completions) {
                    let Some(completion) = completion else {
                        continue;
                    };
                    values.insert(field.response_key, completion.value);
                    errors.extend(completion.errors);
                    bubble |= completion.bubble;
                }
                if bubble {
                    return SelectionResult {
                        value: Object::new(),
                        errors,
                        bubble: true,
                    };
                }
            }
            SelectionResult {
                value: values,
                errors,
                bubble: false,
            }
        })
    }

    async fn execute_normal<'a>(
        &'a self,
        fields: &'a [super::CollectedField<'a>],
        declared_type: &'a str,
        runtime_type: &'a str,
        parent: Option<&'a ResolvedObject>,
        path: &'a [PathSegment],
    ) -> Vec<Option<Completion>> {
        let maximum = self.limits.max_concurrent_fields.max(1);
        let mut next = 0usize;
        let mut active = Vec::new();
        let mut results = (0..fields.len()).map(|_| None).collect::<Vec<_>>();
        std::future::poll_fn(|context| {
            loop {
                while next < fields.len() && active.len() < maximum {
                    let index = next;
                    next += 1;
                    active.push((
                        index,
                        self.execute_field(
                            &fields[index],
                            declared_type,
                            runtime_type,
                            parent,
                            path,
                        ),
                    ));
                }
                let mut made_progress = false;
                let mut position = active.len();
                while position > 0 {
                    position -= 1;
                    let ready = {
                        let (_, future) = &mut active[position];
                        std::future::Future::poll(future.as_mut(), context)
                    };
                    if let std::task::Poll::Ready(completion) = ready {
                        made_progress = true;
                        let (index, _) = active.swap_remove(position);
                        let bubble = completion.bubble;
                        results[index] = Some(completion);
                        if bubble {
                            active.clear();
                            return std::task::Poll::Ready(std::mem::take(&mut results));
                        }
                    }
                }
                if next == fields.len() && active.is_empty() {
                    return std::task::Poll::Ready(std::mem::take(&mut results));
                }
                if !made_progress {
                    return std::task::Poll::Pending;
                }
            }
        })
        .await
    }

    fn execute_field<'a>(
        &'a self,
        group: &'a super::CollectedField<'a>,
        _declared_type: &'a str,
        runtime_type: &'a str,
        parent: Option<&'a ResolvedObject>,
        parent_path: &'a [PathSegment],
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            let field = group.fields[0];
            if !self.claim_output_field(parent_path, group.response_key) {
                // The depletion of the retention byte interrupts the entire selection and then stops the key/path replication.
                return Completion::exhausted();
            }
            let mut path = parent_path.to_vec();
            path.push(PathSegment::Field(group.response_key.to_owned()));
            if field.name.value == "__typename" {
                if !self.claim_output_bytes(runtime_type.len()) {
                    return Completion::exhausted();
                }
                return if self.claim_output_node() {
                    Completion::value(Value::String(runtime_type.to_owned()))
                } else {
                    self.output_limit_error(
                        path,
                        self.field_locations(field),
                        true,
                        Some((runtime_type, &field.name.value)),
                        "completion",
                    )
                };
            }
            if matches!(field.name.value.as_str(), "__schema" | "__type") {
                return self.execute_introspection(group, path, runtime_type).await;
            }
            let Some(runtime_definition) = self.schema.type_by_name(runtime_type) else {
                return self.internal_error(
                    field,
                    &path,
                    runtime_type,
                    "The runtime object type is not in the schema.",
                );
            };
            if runtime_definition.kind() != TypeKind::Object {
                return self.internal_error(
                    field,
                    &path,
                    runtime_type,
                    "Runtime parent is not an object.",
                );
            }
            let Some(definition) = runtime_definition.field(&field.name.value) else {
                return self.internal_error(
                    field,
                    &path,
                    runtime_type,
                    "validated field definition is missing",
                );
            };
            if !self.claim_output_node() {
                return self.output_limit_error(
                    path,
                    self.field_locations(field),
                    matches!(
                        definition.ty.kind,
                        crate::language::ast::TypeRefKind::NonNull(_)
                    ),
                    Some((runtime_type, &field.name.value)),
                    "completion",
                );
            }
            let arguments = match self.coerce_arguments(definition, &field.arguments, &path) {
                Ok(arguments) => arguments,
                Err(errors) => {
                    if self.output_bytes_exhausted() {
                        return Completion::exhausted();
                    }
                    return Completion {
                        value: Value::Null,
                        errors,
                        bubble: matches!(
                            definition.ty.kind,
                            crate::language::ast::TypeRefKind::NonNull(_)
                        ),
                    };
                }
            };
            let directives = match super::directive::coerce(self, group.directives.clone(), &path) {
                Ok(directives) => directives,
                Err(errors) => {
                    if self.output_bytes_exhausted() {
                        return Completion::exhausted();
                    }
                    return Completion {
                        value: Value::Null,
                        errors,
                        bubble: matches!(
                            definition.ty.kind,
                            crate::language::ast::TypeRefKind::NonNull(_)
                        ),
                    };
                }
            };
            let selection_sets = group
                .fields
                .iter()
                .filter_map(|field| field.selection_set.as_ref())
                .collect::<Vec<_>>();
            let metadata = SelectionMetadata {
                response_key: group.response_key,
                has_subselection: !selection_sets.is_empty(),
            };
            let context = FieldContext::execution(
                self.context,
                runtime_type,
                &field.name.value,
                &path,
                &arguments.public,
                metadata,
                &directives,
                Some(self.identity),
                self.telemetry,
            )
            .__with_parent(parent);
            let result = FieldNext::new(self.plugins, move |_| {
                self.dispatch.resolve(
                    self.context,
                    runtime_type,
                    &field.name.value,
                    parent,
                    arguments.dispatch,
                )
            })
            .run(context)
            .await;
            self.complete_value(
                &definition.ty,
                result,
                selection_sets,
                path,
                self.field_locations(field),
                (runtime_type, &field.name.value),
            )
            .await
        })
    }

    fn internal_error(
        &self,
        field: &Field,
        path: &[PathSegment],
        runtime_type: &str,
        message: &str,
    ) -> Completion {
        let error = crate::FieldError::internal(ExecutorInvariant(message.to_owned()));
        Completion::null_with_error(self.graphql_error(
            error,
            self.field_locations(field),
            path.to_vec(),
            Some((runtime_type, &field.name.value)),
            "executor",
        ))
    }

    pub(super) fn field_locations(&self, field: &Field) -> Vec<crate::Location> {
        self.source
            .location(field.name.span.start)
            .into_iter()
            .collect()
    }
}

#[derive(Debug)]
struct ExecutorInvariant(String);

impl std::fmt::Display for ExecutorInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExecutorInvariant {}
