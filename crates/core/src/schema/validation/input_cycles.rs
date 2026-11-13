//! Iteratively detects cycles composed only of required singular input fields.

use crate::Span;
use crate::language::ast::{TypeRef, TypeRefKind};

use super::super::TypeKind;
use super::super::composer::Composer;

#[derive(Clone, Copy)]
struct Edge {
    target: usize,
    span: Span,
}

struct Frame {
    node: usize,
    next_edge: usize,
}

pub(super) fn validate(composer: &mut Composer) {
    let adjacency = build_graph(composer);
    reject_cycles(composer, &adjacency);
}

fn build_graph(composer: &Composer) -> Vec<Vec<Edge>> {
    let mut adjacency = vec![Vec::new(); composer.schema.types.len()];
    for (source, edges) in adjacency.iter_mut().enumerate() {
        let named_type = &composer.schema.types[source];
        if named_type.kind != TypeKind::InputObject {
            continue;
        }
        for field in &named_type.input_fields {
            let Some(target_name) = required_singular_name(&field.ty) else {
                continue;
            };
            let Some(target) = composer.schema.type_index.get(target_name).copied() else {
                continue;
            };
            if composer.schema.types[target].kind == TypeKind::InputObject {
                edges.push(Edge {
                    target,
                    span: field.span,
                });
            }
        }
    }
    adjacency
}

fn required_singular_name(reference: &TypeRef) -> Option<&str> {
    let TypeRefKind::NonNull(inner) = &reference.kind else {
        return None;
    };
    let mut current = inner.as_ref();
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return Some(&name.value),
            // The AST is made from outside the parser and is not returned to the parser.
            TypeRefKind::NonNull(next) => current = next.as_ref(),
            TypeRefKind::List(_) => return None,
        }
    }
}

fn reject_cycles(composer: &mut Composer, adjacency: &[Vec<Edge>]) {
    let mut colors = vec![0_u8; adjacency.len()];
    for start in 0..adjacency.len() {
        if colors[start] != 0 || composer.schema.types[start].kind != TypeKind::InputObject {
            continue;
        }
        colors[start] = 1;
        let mut stack = vec![Frame {
            node: start,
            next_edge: 0,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next_edge == adjacency[frame.node].len() {
                colors[frame.node] = 2;
                stack.pop();
                continue;
            }
            let edge = adjacency[frame.node][frame.next_edge];
            frame.next_edge += 1;
            match colors[edge.target] {
                0 => {
                    colors[edge.target] = 1;
                    stack.push(Frame {
                        node: edge.target,
                        next_edge: 0,
                    });
                }
                1 => {
                    composer.report(
                        super::super::SCHEMA_NON_NULL_INPUT_CYCLE,
                        "Input objects must not form a cycle through non-null singular fields",
                        edge.span,
                    );
                    if !composer.can_continue() {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}
