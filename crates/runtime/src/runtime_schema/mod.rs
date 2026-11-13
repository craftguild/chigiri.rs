//! Convert generated descriptors to a core semantic schema without reparsing SDL.

mod directives;
mod input;
mod types;

#[cfg(test)]
mod tests;

use chigiri_core::language::ast::{
    Definition, Document, OperationType, RootOperationTypeDefinition, SchemaDefinition, Spanned,
    TypeSystemDefinition,
};
use chigiri_core::schema::{SemanticSchema, compose_schema};
use chigiri_core::{ExecutionLimits, SourceId, Span};

use crate::SchemaDescriptor;

pub(crate) fn build(descriptor: &'static SchemaDescriptor) -> Result<SemanticSchema, String> {
    let mut cursor = SpanCursor::new();
    let mut definitions = Vec::new();
    definitions.push(Definition::TypeSystem(schema_definition(
        descriptor,
        &mut cursor,
    )));
    for definition in &descriptor.directives {
        definitions.push(Definition::TypeSystem(TypeSystemDefinition::Directive(
            directives::definition(definition, &mut cursor)?,
        )));
    }
    for ty in &descriptor.types {
        definitions.push(Definition::TypeSystem(types::definition(ty, &mut cursor)?));
    }
    for overlay in &descriptor.built_in_scalar_overlays {
        definitions.push(Definition::TypeSystem(types::scalar_overlay(
            overlay,
            &mut cursor,
        )?));
    }
    let document = Document {
        definitions,
        span: Span::new(SourceId::new(0), 0, cursor.position),
    };
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    output.schema.ok_or_else(|| {
        output
            .diagnostics
            .into_vec()
            .into_iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

fn schema_definition(
    descriptor: &SchemaDescriptor,
    cursor: &mut SpanCursor,
) -> TypeSystemDefinition {
    let mut roots = Vec::new();
    roots.push(root_operation(
        OperationType::Query,
        descriptor.roots.query,
        cursor,
    ));
    if let Some(name) = descriptor.roots.mutation {
        roots.push(root_operation(OperationType::Mutation, name, cursor));
    }
    if let Some(name) = descriptor.roots.subscription {
        roots.push(root_operation(OperationType::Subscription, name, cursor));
    }
    let span = cursor.span();
    TypeSystemDefinition::Schema(SchemaDefinition {
        description: cursor.description(descriptor.description),
        directives: directives::applications(&descriptor.schema_directives, cursor),
        root_operations: roots,
        span,
    })
}

fn root_operation(
    operation_type: OperationType,
    name: &str,
    cursor: &mut SpanCursor,
) -> RootOperationTypeDefinition {
    let span = cursor.span();
    RootOperationTypeDefinition {
        operation_type: Spanned::new(operation_type, span),
        named_type: cursor.name(name),
        span,
    }
}

pub(super) struct SpanCursor {
    position: usize,
}

impl SpanCursor {
    const fn new() -> Self {
        Self { position: 0 }
    }

    fn span(&mut self) -> Span {
        let start = self.position;
        self.position = self.position.saturating_add(1);
        Span::new(SourceId::new(0), start, self.position)
    }

    fn name(&mut self, value: &str) -> chigiri_core::language::ast::Name {
        chigiri_core::language::ast::Name::new(value.to_owned(), self.span())
    }

    fn description(
        &mut self,
        value: Option<&str>,
    ) -> Option<chigiri_core::language::ast::Description> {
        value.map(|value| {
            chigiri_core::language::ast::Description::new(
                chigiri_core::language::ast::StringValue::new(
                    value,
                    chigiri_core::language::ast::StringStyle::Quoted,
                ),
                self.span(),
            )
        })
    }
}
