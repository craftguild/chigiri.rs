//! Converts custom executable directives into middleware invocation metadata.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::executable::ValidatedDocument;
use crate::executable::coerce_argument_with_budget;
use crate::language::ast::{Directive, OperationDefinition, Selection, SelectionSet};
use crate::{GraphQlError, Object, PathSegment};

use super::engine::Engine;

/// One custom executable-directive application with coerced arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutableDirective {
    name: String,
    arguments: Object,
    site: ExecutableDirectiveSite,
}

/// Syntactic location where an executable-directive application appears.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableDirectiveSite {
    /// selected operation definition.
    Operation,
    /// variable definition.
    VariableDefinition { variable: String },
    /// field selection.
    Field { field: String },
    /// named fragment definition.
    FragmentDefinition { fragment: String },
    /// named fragment spread.
    FragmentSpread { fragment: String },
    /// inline fragment.
    InlineFragment,
}

impl ExecutableDirective {
    /// Returns the directive name without the leading `@`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the schema default used argument.
    pub const fn arguments(&self) -> &Object {
        &self.arguments
    }

    /// Returns the executable location that the application describes.
    pub const fn site(&self) -> &ExecutableDirectiveSite {
        &self.site
    }
}

#[derive(Clone)]
pub(crate) struct DirectiveSource<'a> {
    pub(crate) directive: &'a Directive,
    pub(crate) site: Arc<ExecutableDirectiveSite>,
}

pub(crate) fn coerce<'a, C>(
    engine: &Engine<'_, C>,
    directives: impl IntoIterator<Item = DirectiveSource<'a>>,
    path: &[PathSegment],
) -> Result<Vec<ExecutableDirective>, Vec<GraphQlError>> {
    let mut metadata = Vec::new();
    let mut errors = Vec::new();
    for source in directives {
        let directive = source.directive;
        let Some(definition) = engine
            .schema
            .directive_definition(&directive.name.value)
            .and_then(|definition| definition.source_definition())
        else {
            // built-in @skip/@include's semantics are evaluated directly by the collector.
            continue;
        };
        let site_bytes = match source.site.as_ref() {
            ExecutableDirectiveSite::VariableDefinition { variable } => variable.len(),
            ExecutableDirectiveSite::Field { field } => field.len(),
            ExecutableDirectiveSite::FragmentDefinition { fragment }
            | ExecutableDirectiveSite::FragmentSpread { fragment } => fragment.len(),
            _ => 0,
        };
        if !engine.claim_output_bytes(site_bytes)
            || !engine.claim_output_bytes(directive.name.value.len())
        {
            return Err(vec![
                GraphQlError::new("Executable metadata resource limit").code("BAD_USER_INPUT"),
            ]);
        }
        let provided = directive
            .arguments
            .iter()
            .map(|argument| (argument.name.value.as_str(), &argument.value))
            .collect::<BTreeMap<_, _>>();
        let mut arguments = Object::with_capacity(definition.arguments.len());
        for argument in &definition.arguments {
            let output = coerce_argument_with_budget(
                engine.schema,
                argument,
                provided.get(argument.name.value.as_str()).copied(),
                engine.variables,
                engine.scalars,
                &engine.limits,
                engine.input_budget,
            );
            match output.value {
                Some(value) => {
                    if let Some(value) = value.into_value() {
                        arguments.insert(argument.name.value.clone(), value);
                    }
                }
                None => {
                    errors.extend(super::input_errors::rejection(
                        output.diagnostics,
                        engine,
                        path,
                    ));
                    if engine.input_budget.is_some_and(|budget| budget.exhausted())
                        || engine.output_bytes_exhausted()
                    {
                        return Err(errors);
                    }
                }
            }
        }
        metadata.push(ExecutableDirective {
            name: directive.name.value.clone(),
            arguments,
            site: source.site.as_ref().clone(),
        });
    }
    if errors.is_empty() {
        Ok(metadata)
    } else {
        Err(errors)
    }
}

pub(crate) fn operation_sources<'a>(
    document: &'a ValidatedDocument,
    operation: &'a OperationDefinition,
) -> Vec<DirectiveSource<'a>> {
    let mut output = Vec::new();
    for variable in &operation.variable_definitions {
        extend(
            &mut output,
            &variable.directives,
            ExecutableDirectiveSite::VariableDefinition {
                variable: variable.variable.value.clone(),
            },
        );
    }
    extend(
        &mut output,
        &operation.directives,
        ExecutableDirectiveSite::Operation,
    );
    let mut work = Vec::new();
    push_selections(&mut work, &operation.selection_set);
    let mut visited_fragments = BTreeSet::new();
    while let Some(selection) = work.pop() {
        match selection {
            Selection::Field(field) => {
                extend(
                    &mut output,
                    &field.directives,
                    ExecutableDirectiveSite::Field {
                        field: field.name.value.clone(),
                    },
                );
                if let Some(selection_set) = &field.selection_set {
                    push_selections(&mut work, selection_set);
                }
            }
            Selection::FragmentSpread(spread) => {
                extend(
                    &mut output,
                    &spread.directives,
                    ExecutableDirectiveSite::FragmentSpread {
                        fragment: spread.name.value.clone(),
                    },
                );
                if visited_fragments.insert(spread.name.value.clone())
                    && let Some(fragment) = document.fragment(&spread.name.value)
                {
                    extend(
                        &mut output,
                        &fragment.directives,
                        ExecutableDirectiveSite::FragmentDefinition {
                            fragment: fragment.name.value.clone(),
                        },
                    );
                    push_selections(&mut work, &fragment.selection_set);
                }
            }
            Selection::InlineFragment(fragment) => {
                extend(
                    &mut output,
                    &fragment.directives,
                    ExecutableDirectiveSite::InlineFragment,
                );
                push_selections(&mut work, &fragment.selection_set);
            }
        }
    }
    output
}

fn extend<'a>(
    output: &mut Vec<DirectiveSource<'a>>,
    directives: &'a [Directive],
    site: ExecutableDirectiveSite,
) {
    let site = Arc::new(site);
    output.extend(directives.iter().map(|directive| DirectiveSource {
        directive,
        site: site.clone(),
    }));
}

fn push_selections<'a>(work: &mut Vec<&'a Selection>, selection_set: &'a SelectionSet) {
    work.extend(selection_set.selections.iter().rev());
}
