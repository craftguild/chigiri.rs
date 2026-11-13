//! Type completion for introspection values under the shared byte budget.

use super::model::Output;
use crate::executor::BoxFuture;
use crate::executor::engine::{Completion, Engine};
use crate::language::ast::SelectionSet;
use crate::schema::introspection::model::{TypeRefDescriptor, TypeWrapper};
use crate::{Location, PathSegment, Value};

#[derive(Clone)]
pub(super) struct CompletionContext<'a> {
    pub(super) selection_sets: Vec<&'a SelectionSet>,
    pub(super) path: Vec<PathSegment>,
    pub(super) locations: Vec<Location>,
    pub(super) coordinate: (&'a str, &'a str),
}

impl<C: Send + Sync + 'static> Engine<'_, C> {
    pub(super) fn complete_introspection<'a>(
        &'a self,
        output: Output<'a>,
        ty: TypeRefDescriptor,
        layer: usize,
        context: CompletionContext<'a>,
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            match ty.wrappers.get(layer) {
                Some(TypeWrapper::NonNull) => {
                    if !self.claim_output_path(&context.path) {
                        return Completion::exhausted();
                    }
                    let mut completion = self
                        .complete_introspection(output, ty, layer + 1, context.clone())
                        .await;
                    if self.output_bytes_exhausted() {
                        return Completion::exhausted();
                    }
                    if completion.value == Value::Null {
                        if completion.errors.is_empty() {
                            completion.errors.push(self.introspection_error(
                                "non-null introspection field resolved to null",
                                context.path,
                                context.locations,
                                context.coordinate,
                            ));
                        }
                        completion.bubble = true;
                    }
                    completion
                }
                Some(TypeWrapper::List) => {
                    self.complete_introspection_list(output, ty, layer + 1, context)
                        .await
                }
                None => match output {
                    Output::Null => Completion::value(Value::Null),
                    Output::Leaf(value) => {
                        if self.claim_output_leaf(&value) {
                            Completion::value(value)
                        } else {
                            Completion::exhausted()
                        }
                    }
                    Output::Text(text) => {
                        if self.claim_output_bytes(text.len()) {
                            Completion::value(Value::from(text))
                        } else {
                            Completion::exhausted()
                        }
                    }
                    Output::Default(value) => {
                        let mut writer = DefaultWriter {
                            engine: self,
                            value: String::new(),
                        };
                        if super::metadata::serialize_default(value, &mut writer).is_err() {
                            Completion::exhausted()
                        } else {
                            Completion::value(Value::String(writer.value))
                        }
                    }
                    Output::Node(node) => {
                        let nested = self
                            .execute_node(node, context.selection_sets, context.path)
                            .await;
                        Completion {
                            value: if nested.bubble {
                                Value::Null
                            } else {
                                Value::Object(nested.value)
                            },
                            errors: nested.errors,
                            bubble: false,
                        }
                    }
                    Output::List(_) => Completion::null_with_error(self.introspection_error(
                        "the introspection output type does not match the field type",
                        context.path,
                        context.locations,
                        context.coordinate,
                    )),
                },
            }
        })
    }

    fn complete_introspection_list<'a>(
        &'a self,
        output: Output<'a>,
        ty: TypeRefDescriptor,
        item_layer: usize,
        context: CompletionContext<'a>,
    ) -> BoxFuture<'a, Completion> {
        Box::pin(async move {
            let Output::List(items) = output else {
                return if matches!(output, Output::Null) {
                    Completion::value(Value::Null)
                } else {
                    Completion::null_with_error(self.introspection_error(
                        "The introspection list field didn't return the list.",
                        context.path,
                        context.locations,
                        context.coordinate,
                    ))
                };
            };
            if items.len() > self.limits.max_list_items_per_field {
                return self.output_limit_error(
                    context.path,
                    context.locations,
                    false,
                    Some(context.coordinate),
                    "introspection",
                );
            }
            let mut values = Vec::with_capacity(items.len());
            let mut errors = Vec::new();
            for (index, item) in items.into_iter().enumerate() {
                if !self.claim_output_path(&context.path) {
                    return Completion::exhausted();
                }
                let mut item_path = context.path.clone();
                item_path.push(PathSegment::Index(index));
                if !self.claim_output_node() {
                    return self.output_limit_error(
                        item_path,
                        context.locations,
                        false,
                        Some(context.coordinate),
                        "introspection",
                    );
                }
                let completion = self
                    .complete_introspection(
                        item,
                        ty,
                        item_layer,
                        CompletionContext {
                            selection_sets: context.selection_sets.clone(),
                            path: item_path,
                            locations: context.locations.clone(),
                            coordinate: context.coordinate,
                        },
                    )
                    .await;
                if self.output_bytes_exhausted() {
                    return Completion::exhausted();
                }
                values.push(completion.value);
                errors.extend(completion.errors);
                if completion.bubble {
                    return Completion {
                        value: Value::Null,
                        errors,
                        bubble: false,
                    };
                }
            }
            Completion {
                value: Value::List(values),
                errors,
                bubble: false,
            }
        })
    }
}

struct DefaultWriter<'a, 'e, C> {
    engine: &'a Engine<'e, C>,
    value: String,
}
impl<C> std::fmt::Write for DefaultWriter<'_, '_, C> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if !self.engine.claim_output_bytes(text.len()) {
            return Err(std::fmt::Error);
        }
        self.value.push_str(text);
        Ok(())
    }
}
