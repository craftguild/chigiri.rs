//! Input object dependency graph to generic propagation and return edge to box objects.

use ::std::collections::{BTreeMap, BTreeSet, VecDeque};

use chigiri_core::language::ast::{TypeRef, TypeRefKind};
use chigiri_core::schema::{SemanticSchema, TypeKind};

use super::user_types;

pub(super) struct InputGraphAnalysis {
    pub(super) generics: BTreeSet<String>,
    pub(super) boxed_edges: BTreeSet<(String, String)>,
}

pub(super) fn analyze_input_graph(
    schema: &SemanticSchema,
    bound_enums: &BTreeSet<String>,
) -> InputGraphAnalysis {
    let inputs = user_types(schema)
        .filter(|ty| ty.kind() == TypeKind::InputObject)
        .collect::<Vec<_>>();
    let indices = inputs
        .iter()
        .enumerate()
        .map(|(index, ty)| (ty.name(), index))
        .collect::<BTreeMap<_, _>>();
    let custom_scalars = user_types(schema)
        .filter(|ty| ty.kind() == TypeKind::Scalar)
        .map(|ty| ty.name())
        .collect::<BTreeSet<_>>();
    let mut all_edges = vec![Vec::new(); inputs.len()];
    let mut direct_edges = vec![Vec::new(); inputs.len()];
    let mut scalar_owners = Vec::new();
    for (owner, ty) in inputs.iter().enumerate() {
        for field in ty.input_fields() {
            let target = named_type(&field.ty);
            if custom_scalars.contains(target) || bound_enums.contains(target) {
                scalar_owners.push(owner);
            }
            if let Some(target) = indices.get(target).copied() {
                all_edges[owner].push(target);
                if is_direct_singular(&field.ty) {
                    direct_edges[owner].push((target, field.name.value.as_str()));
                }
            }
        }
    }
    InputGraphAnalysis {
        generics: propagate_generics(&inputs, &all_edges, scalar_owners),
        boxed_edges: cyclic_edges(&inputs, &direct_edges),
    }
}

fn propagate_generics(
    inputs: &[&chigiri_core::schema::SemanticType],
    edges: &[Vec<usize>],
    initial: Vec<usize>,
) -> BTreeSet<String> {
    let mut reverse = vec![Vec::new(); inputs.len()];
    for (owner, targets) in edges.iter().enumerate() {
        for target in targets {
            reverse[*target].push(owner);
        }
    }
    let mut generic = vec![false; inputs.len()];
    let mut queue = VecDeque::new();
    for owner in initial {
        if !generic[owner] {
            generic[owner] = true;
            queue.push_back(owner);
        }
    }
    while let Some(target) = queue.pop_front() {
        for owner in &reverse[target] {
            if !generic[*owner] {
                generic[*owner] = true;
                queue.push_back(*owner);
            }
        }
    }
    inputs
        .iter()
        .enumerate()
        .filter(|(index, _)| generic[*index])
        .map(|(_, ty)| ty.name().to_owned())
        .collect()
}

fn cyclic_edges(
    inputs: &[&chigiri_core::schema::SemanticType],
    edges: &[Vec<(usize, &str)>],
) -> BTreeSet<(String, String)> {
    let adjacency = edges
        .iter()
        .map(|targets| {
            targets
                .iter()
                .map(|(target, _)| *target)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let components = strongly_connected_components(&adjacency);
    edges
        .iter()
        .enumerate()
        .flat_map(|(owner, targets)| {
            let components = &components;
            targets
                .iter()
                .filter(move |(target, _)| components[owner] == components[*target])
                .map(move |(_, field)| (inputs[owner].name().to_owned(), (*field).to_owned()))
        })
        .collect()
}

fn strongly_connected_components(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut reverse = vec![Vec::new(); edges.len()];
    for (owner, targets) in edges.iter().enumerate() {
        for target in targets {
            reverse[*target].push(owner);
        }
    }
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for start in 0..edges.len() {
        finish_order(start, edges, &mut visited, &mut order);
    }
    let mut components = vec![usize::MAX; edges.len()];
    let mut component = 0usize;
    for start in order.into_iter().rev() {
        if components[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        components[start] = component;
        while let Some(node) = stack.pop() {
            for next in &reverse[node] {
                if components[*next] == usize::MAX {
                    components[*next] = component;
                    stack.push(*next);
                }
            }
        }
        component += 1;
    }
    components
}

fn finish_order(start: usize, edges: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
    if visited[start] {
        return;
    }
    visited[start] = true;
    let mut stack = vec![(start, 0usize)];
    while let Some((node, next_index)) = stack.last_mut() {
        if let Some(next) = edges[*node].get(*next_index).copied() {
            *next_index += 1;
            if !visited[next] {
                visited[next] = true;
                stack.push((next, 0));
            }
        } else {
            order.push(*node);
            stack.pop();
        }
    }
}

fn named_type(mut ty: &TypeRef) -> &str {
    loop {
        match &ty.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => ty = inner,
        }
    }
}

fn is_direct_singular(mut ty: &TypeRef) -> bool {
    loop {
        match &ty.kind {
            TypeRefKind::Named(_) => return true,
            TypeRefKind::NonNull(inner) => ty = inner,
            TypeRefKind::List(_) => return false,
        }
    }
}
