//! Fragment-definition catalog and spread-graph cycle validation.

use std::collections::BTreeMap;

use crate::language::ast::{Definition, Document, ExecutableDefinition, FragmentDefinition};
use crate::{Diagnostic, DiagnosticBag, Span};

use super::{FRAGMENT_SPREAD_CYCLE, FRAGMENT_UNUSED};

#[derive(Clone, Copy)]
struct Edge {
    target: usize,
    span: Span,
}

pub(super) struct FragmentCatalog<'document> {
    definitions: Vec<&'document FragmentDefinition>,
    indices: BTreeMap<&'document str, usize>,
}

impl<'document> FragmentCatalog<'document> {
    pub(super) fn new(document: &'document Document) -> Self {
        let mut definitions = Vec::new();
        let mut indices = BTreeMap::new();
        for definition in &document.definitions {
            let Definition::Executable(ExecutableDefinition::Fragment(fragment)) = definition
            else {
                continue;
            };
            if let std::collections::btree_map::Entry::Vacant(entry) =
                indices.entry(fragment.name.value.as_str())
            {
                entry.insert(definitions.len());
                definitions.push(fragment);
            }
        }
        Self {
            definitions,
            indices,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.definitions.len()
    }

    pub(super) fn index(&self, name: &str) -> Option<usize> {
        self.indices.get(name).copied()
    }

    pub(super) fn definition(&self, index: usize) -> &'document FragmentDefinition {
        self.definitions[index]
    }

    pub(super) fn is_primary(&self, index: usize, fragment: &FragmentDefinition) -> bool {
        std::ptr::eq(self.definitions[index], fragment)
    }
}

pub(super) struct SpreadGraph {
    edges: Vec<Vec<Edge>>,
    operation_node: usize,
}

impl SpreadGraph {
    pub(super) fn new(fragment_count: usize) -> Self {
        Self {
            edges: vec![Vec::new(); fragment_count + 1],
            operation_node: fragment_count,
        }
    }

    pub(super) const fn operation_node(&self) -> usize {
        self.operation_node
    }

    pub(super) fn add(&mut self, owner: usize, target: usize, span: Span) {
        self.edges[owner].push(Edge { target, span });
    }

    pub(super) fn validate(
        &self,
        catalog: &FragmentCatalog<'_>,
        diagnostics: &mut DiagnosticBag,
    ) -> bool {
        let mut valid = true;
        if !self.validate_cycles(diagnostics, &mut valid) {
            return false;
        }
        if !self.validate_usage(catalog, diagnostics, &mut valid) {
            return false;
        }
        valid
    }

    fn validate_cycles(&self, diagnostics: &mut DiagnosticBag, valid: &mut bool) -> bool {
        let graph = &self.edges[..self.operation_node];
        let mut colors = vec![0_u8; graph.len()];
        for start in 0..graph.len() {
            if colors[start] != 0 {
                continue;
            }
            colors[start] = 1;
            let mut stack = vec![(start, 0_usize)];
            while let Some((node, cursor)) = stack.last_mut() {
                let node_index = *node;
                let Some(edge) = graph[node_index].get(*cursor).copied() else {
                    colors[node_index] = 2;
                    stack.pop();
                    continue;
                };
                *cursor += 1;
                match colors[edge.target] {
                    0 => {
                        colors[edge.target] = 1;
                        stack.push((edge.target, 0));
                    }
                    1 => {
                        *valid = false;
                        if !diagnostics.push(Diagnostic::new(
                            FRAGMENT_SPREAD_CYCLE,
                            "Fragment spreads must not form cycles",
                            edge.span,
                        )) || diagnostics.should_stop()
                        {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    }

    fn validate_usage(
        &self,
        catalog: &FragmentCatalog<'_>,
        diagnostics: &mut DiagnosticBag,
        valid: &mut bool,
    ) -> bool {
        let mut used = vec![false; self.operation_node];
        let mut worklist = self.edges[self.operation_node]
            .iter()
            .map(|edge| edge.target)
            .collect::<Vec<_>>();
        while let Some(index) = worklist.pop() {
            if used[index] {
                continue;
            }
            used[index] = true;
            worklist.extend(self.edges[index].iter().map(|edge| edge.target));
        }
        for (index, used) in used.into_iter().enumerate() {
            if used {
                continue;
            }
            let fragment = catalog.definition(index);
            *valid = false;
            if !diagnostics.push(Diagnostic::new(
                FRAGMENT_UNUSED,
                format!("Fragment `{}` is not used", fragment.name.value),
                fragment.name.span,
            )) || diagnostics.should_stop()
            {
                return false;
            }
        }
        true
    }
}
