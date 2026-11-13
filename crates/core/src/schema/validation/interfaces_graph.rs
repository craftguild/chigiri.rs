//! Validates interface targets, extensions, cycles, and transitive implementation after merging.

use crate::Span;
use std::collections::BTreeSet;

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
    if composer.can_continue() {
        reject_cycles(composer, &adjacency);
    }
    if composer.can_continue() {
        validate_transitive_closure(composer);
    }
}

fn build_graph(composer: &mut Composer) -> Vec<Vec<Edge>> {
    let mut adjacency = vec![Vec::new(); composer.schema.types.len()];
    for (index, edges) in adjacency.iter_mut().enumerate() {
        if !composer.can_continue() {
            break;
        }
        let named_type = composer.schema.types[index].clone();
        if !matches!(named_type.kind, TypeKind::Object | TypeKind::Interface) {
            continue;
        }
        let mut seen = BTreeSet::new();
        for implemented in named_type.implements_interfaces {
            if !seen.insert(implemented.value.clone()) {
                composer.report(
                    super::super::SCHEMA_DUPLICATE_MEMBER,
                    format!(
                        "Type `{}` implements interface `{}` more than once",
                        named_type.name, implemented.value
                    ),
                    implemented.span,
                );
                if !composer.can_continue() {
                    break;
                }
                continue;
            }
            let Some(target) = interface_index(composer, &implemented.value) else {
                composer.report(
                    super::super::SCHEMA_INVALID_INTERFACE_TARGET,
                    format!(
                        "Type `{}` can only implement a defined Interface, not `{}`",
                        named_type.name, implemented.value
                    ),
                    implemented.span,
                );
                if !composer.can_continue() {
                    break;
                }
                continue;
            };
            if target == index {
                composer.report(
                    super::super::SCHEMA_INTERFACE_SELF_IMPLEMENTATION,
                    format!("Interface `{}` cannot implement itself", named_type.name),
                    implemented.span,
                );
                if !composer.can_continue() {
                    break;
                }
                continue;
            }
            if named_type.kind == TypeKind::Interface {
                edges.push(Edge {
                    target,
                    span: implemented.span,
                });
            }
        }
    }
    adjacency
}

fn interface_index(composer: &Composer, name: &str) -> Option<usize> {
    let index = composer.schema.type_index.get(name).copied()?;
    (composer.schema.types[index].kind == TypeKind::Interface).then_some(index)
}

fn reject_cycles(composer: &mut Composer, adjacency: &[Vec<Edge>]) {
    let mut colors = vec![0_u8; adjacency.len()];
    for start in 0..adjacency.len() {
        if colors[start] != 0 || composer.schema.types[start].kind != TypeKind::Interface {
            continue;
        }
        colors[start] = 1;
        let mut stack = vec![Frame {
            node: start,
            next_edge: 0,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next_edge >= adjacency[frame.node].len() {
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
                        super::super::SCHEMA_INTERFACE_CYCLE,
                        "Interface implementation graph must not contain a cycle",
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

fn validate_transitive_closure(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let implementor = composer.schema.types[index].clone();
        if !matches!(implementor.kind, TypeKind::Object | TypeKind::Interface) {
            continue;
        }
        let declared = implementor
            .implements_interfaces
            .iter()
            .map(|name| name.value.as_str())
            .collect::<BTreeSet<_>>();
        let mut processed = BTreeSet::new();
        let mut reported_missing = BTreeSet::new();
        for direct in &implementor.implements_interfaces {
            if !processed.insert(direct.value.as_str()) {
                continue;
            }
            let Some(interface) = composer.schema.type_by_name(&direct.value) else {
                continue;
            };
            if interface.kind() != TypeKind::Interface {
                continue;
            }
            let transitives = interface.implemented_interfaces().to_vec();
            for transitive in transitives {
                if declared.contains(transitive.value.as_str())
                    || !reported_missing.insert(transitive.value.clone())
                {
                    continue;
                }
                composer.report(
                    super::super::SCHEMA_TRANSITIVE_INTERFACE_MISSING,
                    format!(
                        "Type `{}` must explicitly implement transitive interface `{}`",
                        implementor.name, transitive.value
                    ),
                    direct.span,
                );
                if !composer.can_continue() {
                    return;
                }
            }
        }
    }
}
