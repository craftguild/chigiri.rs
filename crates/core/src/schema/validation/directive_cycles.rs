//! Iteratively detect directive-definition cycles through input types and directives.

use crate::Span;
use crate::language::ast::{Directive, TypeRef, TypeRefKind};

use super::super::composer::Composer;
use super::super::{SemanticTypeRef, SemanticTypeRefKind, TypeKind};

#[derive(Clone, Copy)]
struct Edge {
    target: usize,
    directive_span: Option<Span>,
}

pub(super) fn validate(composer: &mut Composer) {
    let directive_count = composer.schema.directives.len();
    let graph = build_graph(composer, directive_count);
    let adjacency = graph
        .iter()
        .map(|edges| edges.iter().map(|edge| edge.target).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let components = super::scc::strongly_connected_components(&adjacency);
    reject_directive_cycles(composer, &graph, &components, directive_count);
}

fn build_graph(composer: &Composer, directive_count: usize) -> Vec<Vec<Edge>> {
    let mut graph = vec![Vec::new(); directive_count + composer.schema.types.len()];
    let (directive_graph, type_graph) = graph.split_at_mut(directive_count);
    for (edges, definition) in directive_graph.iter_mut().zip(&composer.schema.directives) {
        for argument in &definition.arguments {
            if let Some(source) = &argument.source_definition {
                add_directive_edges(composer, edges, &source.directives);
            }
            add_semantic_type_edge(composer, edges, &argument.ty, directive_count);
        }
    }
    for (edges, named_type) in type_graph.iter_mut().zip(&composer.schema.types) {
        add_directive_edges(composer, edges, &named_type.directives);
        match named_type.kind {
            TypeKind::InputObject => {
                for field in &named_type.input_fields {
                    add_directive_edges(composer, edges, &field.directives);
                    add_type_edge(composer, edges, &field.ty, directive_count);
                }
            }
            TypeKind::Enum => {
                for value in &named_type.enum_values {
                    add_directive_edges(composer, edges, &value.directives);
                }
            }
            TypeKind::Scalar | TypeKind::Object | TypeKind::Interface | TypeKind::Union => {}
        }
    }
    graph
}

fn add_directive_edges(composer: &Composer, edges: &mut Vec<Edge>, uses: &[Directive]) {
    for application in uses {
        let Some(target) = composer
            .schema
            .directive_index
            .get(&application.name.value)
            .copied()
        else {
            continue;
        };
        edges.push(Edge {
            target,
            directive_span: Some(application.span),
        });
    }
}

fn add_type_edge(
    composer: &Composer,
    edges: &mut Vec<Edge>,
    reference: &TypeRef,
    directive_count: usize,
) {
    let Some(name) = named_type(reference) else {
        return;
    };
    add_named_type_edge(composer, edges, name, directive_count);
}

fn add_semantic_type_edge(
    composer: &Composer,
    edges: &mut Vec<Edge>,
    reference: &SemanticTypeRef,
    directive_count: usize,
) {
    let Some(name) = semantic_named_type(reference) else {
        return;
    };
    add_named_type_edge(composer, edges, name, directive_count);
}

fn add_named_type_edge(
    composer: &Composer,
    edges: &mut Vec<Edge>,
    name: &str,
    directive_count: usize,
) {
    let Some(target) = composer.schema.type_index.get(name).copied() else {
        return;
    };
    if matches!(
        composer.schema.types[target].kind,
        TypeKind::Scalar | TypeKind::Enum | TypeKind::InputObject
    ) {
        edges.push(Edge {
            target: directive_count + target,
            directive_span: None,
        });
    }
}

fn named_type(reference: &TypeRef) -> Option<&str> {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return Some(&name.value),
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner,
        }
    }
}

fn semantic_named_type(reference: &SemanticTypeRef) -> Option<&str> {
    let mut current = reference;
    loop {
        match &current.kind {
            SemanticTypeRefKind::Named(name) => return Some(name),
            SemanticTypeRefKind::List(inner) | SemanticTypeRefKind::NonNull(inner) => {
                current = inner;
            }
        }
    }
}

fn reject_directive_cycles(
    composer: &mut Composer,
    graph: &[Vec<Edge>],
    components: &[usize],
    directive_count: usize,
) {
    let mut reported = vec![false; graph.len()];
    for source in 0..graph.len() {
        for edge in &graph[source] {
            let component = components[source];
            if component != components[edge.target]
                || edge.target >= directive_count
                || reported[component]
            {
                continue;
            }
            let Some(span) = edge.directive_span else {
                continue;
            };
            reported[component] = true;
            composer.report(
                super::super::SCHEMA_DIRECTIVE_DEFINITION_CYCLE,
                "Directive definition must not reference itself directly or indirectly",
                span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}
