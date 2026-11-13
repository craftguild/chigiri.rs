//! Complete resolver output by field type with null propagation to the response root.

use crate::language::ast::{SelectionSet, TypeRef, TypeRefKind};
use crate::schema::TypeKind;
use crate::{FieldError, FieldResult, Location, PathSegment, Value};

use super::engine::{Completion, Engine};
use super::{BoxFuture, ResolvedValue};

impl<C> Engine<'_, C>
where
    C: Send + Sync + 'static,
{
    pub(super) fn complete_value<'a>(
        &'a self,
        ty: &'a TypeRef,
        result: FieldResult<ResolvedValue>,
        selection_sets: Vec<&'a SelectionSet>,
        path: Vec<PathSegment>,
        locations: Vec<Location>,
        coordinate: (&'a str, &'a str),
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            match &ty.kind {
                TypeRefKind::NonNull(inner) => {
                    if !self.claim_output_path(&path) {
                        return Completion::exhausted();
                    }
                    let mut completion = self
                        .complete_nullable(
                            inner,
                            result,
                            selection_sets,
                            path.clone(),
                            locations.clone(),
                            coordinate,
                        )
                        .await;
                    if self.output_bytes_exhausted() {
                        return Completion::exhausted();
                    }
                    if completion.value == Value::Null {
                        if completion.errors.is_empty() {
                            completion.errors.push(self.graphql_error(
                                FieldError::internal(CompletionInvariant(
                                    "non-null field resolved to null".to_owned(),
                                )),
                                locations,
                                path,
                                Some(coordinate),
                                "completion",
                            ));
                        }
                        completion.bubble = true;
                    }
                    completion
                }
                _ => {
                    let mut completion = self
                        .complete_nullable(ty, result, selection_sets, path, locations, coordinate)
                        .await;
                    if completion.bubble {
                        completion.value = Value::Null;
                        completion.bubble = false;
                    }
                    completion
                }
            }
        })
    }

    fn complete_nullable<'a>(
        &'a self,
        ty: &'a TypeRef,
        result: FieldResult<ResolvedValue>,
        selection_sets: Vec<&'a SelectionSet>,
        path: Vec<PathSegment>,
        locations: Vec<Location>,
        coordinate: (&'a str, &'a str),
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            let value = match result {
                Ok(ResolvedValue::Null) => return Completion::value(Value::Null),
                Ok(value) => value,
                Err(error) => {
                    return Completion::null_with_error(self.graphql_error(
                        error,
                        locations,
                        path,
                        Some(coordinate),
                        "resolver",
                    ));
                }
            };
            match (&ty.kind, value) {
                (TypeRefKind::Named(name), ResolvedValue::Leaf(value)) => {
                    if !self.claim_output_leaf(&value) {
                        crate::value::drop_iterative(value);
                        if self.output_bytes_exhausted() {
                            return Completion::exhausted();
                        }
                        return self.output_limit_error(
                            path,
                            locations,
                            false,
                            Some(coordinate),
                            "completion",
                        );
                    }
                    let kind = self.schema.type_by_name(&name.value).map(|ty| ty.kind());
                    if matches!(kind, Some(TypeKind::Enum)) {
                        let ty = self.schema.type_by_name(&name.value).unwrap();
                        match super::output::coerce_enum(ty, value) {
                            Ok(value) => Completion::value(value),
                            Err(message) => {
                                self.completion_error(path, locations, coordinate, message)
                            }
                        }
                    } else if matches!(kind, Some(TypeKind::Scalar))
                        && super::output::is_builtin_scalar(&name.value)
                    {
                        if name.value == "ID"
                            && let Value::Int(integer) = &value
                        {
                            let magnitude = integer.unsigned_abs();
                            let bytes = if magnitude == 0 {
                                1
                            } else {
                                magnitude.ilog10() as usize + 1
                            } + usize::from(*integer < 0);
                            if !self.claim_output_bytes(bytes) {
                                return Completion::exhausted();
                            }
                        }
                        match super::output::coerce_builtin_scalar(&name.value, value) {
                            Ok(value) => Completion::value(value),
                            Err(message) => {
                                self.completion_error(path, locations, coordinate, message)
                            }
                        }
                    } else if matches!(kind, Some(TypeKind::Scalar)) {
                        match super::output::validate_custom_scalar(value, &self.limits) {
                            Ok(validated) => {
                                let (value, additional_nodes) = validated.into_parts();
                                if self.claim_output_nodes(additional_nodes) {
                                    Completion::value(value)
                                } else {
                                    crate::value::drop_iterative(value);
                                    self.output_limit_error(
                                        path,
                                        locations,
                                        false,
                                        Some(coordinate),
                                        "completion",
                                    )
                                }
                            }
                            Err(super::output::CustomScalarOutputError::ResourceLimit) => self
                                .output_limit_error(
                                    path,
                                    locations,
                                    false,
                                    Some(coordinate),
                                    "completion",
                                ),
                            Err(super::output::CustomScalarOutputError::InvalidNumeric) => self
                                .completion_error(
                                    path,
                                    locations,
                                    coordinate,
                                    "The custom-scalar output has an invalid numeric representation.",
                                ),
                        }
                    } else {
                        self.completion_error(
                            path,
                            locations,
                            coordinate,
                            "a composite field returned a leaf value",
                        )
                    }
                }
                (TypeRefKind::Named(name), ResolvedValue::Object(object)) => {
                    let allowed =
                        self.schema
                            .possible_objects(&name.value)
                            .is_some_and(|objects| {
                                objects.iter().any(|ty| ty.as_ref() == object.type_name())
                            });
                    if !allowed || !object.belongs_to(self.dispatch.object_scope()) {
                        return self.completion_error(
                            path,
                            locations,
                            coordinate,
                            "abstract/object field returned the incorrect runtime typename.",
                        );
                    }
                    let nested = self
                        .execute_selection(
                            selection_sets,
                            &name.value,
                            object.type_name(),
                            Some(&object),
                            path,
                            false,
                        )
                        .await;
                    Completion {
                        value: if nested.bubble {
                            Value::Null
                        } else {
                            Value::Object(nested.value)
                        },
                        errors: nested.errors,
                        bubble: nested.bubble,
                    }
                }
                (TypeRefKind::List(item_type), ResolvedValue::List(items)) => {
                    if items.len() > self.limits.max_list_items_per_field {
                        return self.output_limit_error(
                            path,
                            locations,
                            false,
                            Some(coordinate),
                            "completion",
                        );
                    }
                    let mut values = Vec::with_capacity(items.len());
                    let mut errors = Vec::new();
                    for (index, item) in items.into_iter().enumerate() {
                        if !self.claim_output_path(&path) {
                            return Completion::exhausted();
                        }
                        let mut item_path = path.clone();
                        item_path.push(PathSegment::Index(index));
                        if !self.claim_output_node() {
                            return self.output_limit_error(
                                item_path,
                                locations,
                                false,
                                Some(coordinate),
                                "completion",
                            );
                        }
                        let completion = self
                            .complete_value(
                                item_type,
                                item,
                                selection_sets.clone(),
                                item_path,
                                locations.clone(),
                                coordinate,
                            )
                            .await;
                        values.push(completion.value);
                        errors.extend(completion.errors);
                        if completion.bubble {
                            return Completion {
                                value: Value::Null,
                                errors,
                                bubble: true,
                            };
                        }
                    }
                    Completion {
                        value: Value::List(values),
                        errors,
                        bubble: false,
                    }
                }
                (TypeRefKind::NonNull(_), _) => self.completion_error(
                    path,
                    locations,
                    coordinate,
                    "nested non-null completion invariant was violated",
                ),
                _ => self.completion_error(
                    path,
                    locations,
                    coordinate,
                    "The output type of the resolver does not match the field type.",
                ),
            }
        })
    }

    fn completion_error(
        &self,
        path: Vec<PathSegment>,
        locations: Vec<Location>,
        coordinate: (&str, &str),
        message: &str,
    ) -> Completion {
        let error = FieldError::internal(CompletionInvariant(message.to_owned()));
        Completion::null_with_error(self.graphql_error(
            error,
            locations,
            path,
            Some(coordinate),
            "completion",
        ))
    }
}

#[derive(Debug)]
struct CompletionInvariant(String);

impl std::fmt::Display for CompletionInvariant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CompletionInvariant {}
