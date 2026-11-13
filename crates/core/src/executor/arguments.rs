//! Apply variable and default coercion to validated field arguments.

use std::collections::BTreeMap;

use crate::executable::coerce_argument_with_budget;
use crate::language::ast::{Argument, FieldDefinition};
use crate::{GraphQlError, Object, PathSegment};

use super::ArgumentValues;
use super::engine::Engine;

pub(crate) struct CoercedArguments {
    pub(crate) dispatch: ArgumentValues,
    pub(crate) public: Object,
}

impl<C> Engine<'_, C> {
    pub(super) fn coerce_arguments(
        &self,
        definition: &FieldDefinition,
        provided: &[Argument],
        path: &[PathSegment],
    ) -> Result<CoercedArguments, Vec<GraphQlError>> {
        let provided = provided
            .iter()
            .map(|argument| (argument.name.value.as_str(), &argument.value))
            .collect::<BTreeMap<_, _>>();
        let mut dispatch = Vec::with_capacity(definition.arguments.len());
        let mut public = Object::with_capacity(definition.arguments.len());
        let mut errors = Vec::new();
        for argument in &definition.arguments {
            let output = coerce_argument_with_budget(
                self.schema,
                argument,
                provided.get(argument.name.value.as_str()).copied(),
                self.variables,
                self.scalars,
                &self.limits,
                self.input_budget,
            );
            if let Some(value) = output.value {
                if let Some(value) = value.clone().into_value() {
                    public.insert(argument.name.value.clone(), value);
                }
                dispatch.push(value);
            } else {
                errors.extend(super::input_errors::rejection(
                    output.diagnostics,
                    self,
                    path,
                ));
                if self.input_budget.is_some_and(|budget| budget.exhausted())
                    || self.output_bytes_exhausted()
                {
                    return Err(errors);
                }
            }
        }
        if errors.is_empty() {
            Ok(CoercedArguments {
                dispatch: ArgumentValues::new(dispatch),
                public,
            })
        } else {
            Err(errors)
        }
    }
}
