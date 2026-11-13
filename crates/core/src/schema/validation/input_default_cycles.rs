//! Traverses the field-coordinate graph to detect input-object default-value cycles.

use std::collections::{BTreeMap, BTreeSet};

use crate::Span;
use crate::language::ast::{ObjectField, TypeRef, TypeRefKind, Value, ValueNode};

use super::super::composer::Composer;
use super::super::{SemanticSchema, TypeKind};

struct InputField {
    target: Option<usize>,
    default_position: Option<usize>,
}

#[derive(Default)]
struct InputObject {
    fields: BTreeMap<String, InputField>,
    default_nodes: Vec<usize>,
    default_ranges: Option<DefaultRanges>,
}

struct DefaultRanges {
    base: usize,
    nodes: Vec<Option<usize>>,
}

struct DefaultNode {
    owner: usize,
    field: usize,
    target: usize,
    value: ValueNode,
    span: Span,
}

enum Work<'a> {
    Value {
        target: usize,
        value: &'a ValueNode,
    },
    Object {
        target: usize,
        entries: &'a [ObjectField],
    },
}

pub(super) fn validate(composer: &mut Composer) {
    let (mut objects, nodes) = build_catalog(&composer.schema);
    let graph = build_default_graph(&mut objects, &nodes);
    let components = super::scc::strongly_connected_components(&graph);
    reject_cycles(composer, &graph, &components, &nodes);
}

fn build_catalog(schema: &SemanticSchema) -> (Vec<InputObject>, Vec<DefaultNode>) {
    let mut objects = (0..schema.types.len())
        .map(|_| InputObject::default())
        .collect::<Vec<_>>();
    let mut nodes = Vec::new();
    for (owner, named_type) in schema.types.iter().enumerate() {
        if named_type.kind != TypeKind::InputObject {
            continue;
        }
        for (field_index, field) in named_type.input_fields.iter().enumerate() {
            let target = input_object_index(schema, &field.ty);
            let mut default_position = None;
            if let (Some(target), Some(value)) = (target, field.default_value.as_ref()) {
                let node = nodes.len();
                default_position = Some(objects[owner].default_nodes.len());
                nodes.push(DefaultNode {
                    owner,
                    field: field_index,
                    target,
                    value: value.clone(),
                    span: value.span,
                });
                objects[owner].default_nodes.push(node);
            }
            objects[owner].fields.insert(
                field.name.value.clone(),
                InputField {
                    target,
                    default_position,
                },
            );
        }
    }
    (objects, nodes)
}

fn input_object_index(schema: &SemanticSchema, reference: &TypeRef) -> Option<usize> {
    let name = underlying_name(reference);
    let index = schema.type_index.get(name).copied()?;
    (schema.types[index].kind == TypeKind::InputObject).then_some(index)
}

fn underlying_name(reference: &TypeRef) -> &str {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner.as_ref(),
        }
    }
}

fn build_default_graph(objects: &mut [InputObject], nodes: &[DefaultNode]) -> Vec<Vec<usize>> {
    let mut graph = vec![Vec::new(); nodes.len()];
    build_default_ranges(objects, &mut graph);
    for (source, node) in nodes.iter().enumerate() {
        graph[source] = discover_edges(objects, node.target, &node.value);
    }
    graph
}

fn build_default_ranges(objects: &mut [InputObject], graph: &mut Vec<Vec<usize>>) {
    for object in objects {
        let count = object.default_nodes.len();
        if count == 0 {
            continue;
        }
        let base = count
            .checked_next_power_of_two()
            .expect("the reserved vector length must fit in usize");
        let mut range_nodes = vec![None; base * 2];
        for (position, &node) in object.default_nodes.iter().enumerate() {
            range_nodes[base + position] = Some(node);
        }
        for index in (1..base).rev() {
            range_nodes[index] = match (range_nodes[index * 2], range_nodes[index * 2 + 1]) {
                (Some(left), Some(right)) => {
                    let node = graph.len();
                    graph.push(vec![left, right]);
                    Some(node)
                }
                (Some(node), None) | (None, Some(node)) => Some(node),
                (None, None) => None,
            };
        }
        object.default_ranges = Some(DefaultRanges {
            base,
            nodes: range_nodes,
        });
    }
}

fn discover_edges(objects: &[InputObject], target: usize, value: &ValueNode) -> Vec<usize> {
    let mut edges = BTreeSet::new();
    let mut work = vec![Work::Value { target, value }];
    while let Some(event) = work.pop() {
        match event {
            Work::Value { target, value } => push_value(&mut work, target, value),
            Work::Object { target, entries } => {
                inspect_object(objects, &mut edges, &mut work, target, entries);
            }
        }
    }
    edges.into_iter().collect()
}

fn push_value<'a>(work: &mut Vec<Work<'a>>, target: usize, value: &'a ValueNode) {
    match &value.value {
        Value::List(items) => {
            for item in items.iter().rev() {
                work.push(Work::Value {
                    target,
                    value: item,
                });
            }
        }
        Value::Object(entries) => work.push(Work::Object { target, entries }),
        Value::Variable(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Boolean(_)
        | Value::Null
        | Value::Enum(_) => {}
    }
}

fn inspect_object<'a>(
    objects: &[InputObject],
    edges: &mut BTreeSet<usize>,
    work: &mut Vec<Work<'a>>,
    target: usize,
    entries: &'a [ObjectField],
) {
    let object = &objects[target];
    let mut explicit = BTreeMap::new();
    for entry in entries {
        explicit
            .entry(entry.name.value.as_str())
            .or_insert(&entry.value);
    }
    for (&name, &value) in &explicit {
        let Some(field) = object.fields.get(name) else {
            continue;
        };
        if let Some(field_target) = field.target {
            work.push(Work::Value {
                target: field_target,
                value,
            });
        }
    }
    let mut excluded = explicit
        .keys()
        .filter_map(|name| object.fields.get(*name)?.default_position)
        .collect::<Vec<_>>();
    excluded.sort_unstable();
    let mut start = 0;
    for position in excluded {
        add_default_range(object, start, position, edges);
        start = position + 1;
    }
    add_default_range(object, start, object.default_nodes.len(), edges);
}

fn add_default_range(object: &InputObject, start: usize, end: usize, edges: &mut BTreeSet<usize>) {
    let Some(ranges) = &object.default_ranges else {
        return;
    };
    let mut left = ranges.base + start;
    let mut right = ranges.base + end;
    while left < right {
        if left % 2 == 1 {
            if let Some(node) = ranges.nodes[left] {
                edges.insert(node);
            }
            left += 1;
        }
        if right % 2 == 1 {
            right -= 1;
            if let Some(node) = ranges.nodes[right] {
                edges.insert(node);
            }
        }
        left /= 2;
        right /= 2;
    }
}

fn reject_cycles(
    composer: &mut Composer,
    graph: &[Vec<usize>],
    components: &[usize],
    nodes: &[DefaultNode],
) {
    let component_count = components
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum + 1);
    let mut sizes = vec![0_usize; component_count];
    for &component in components {
        sizes[component] += 1;
    }
    for (node, definition) in nodes.iter().enumerate() {
        let component = components[node];
        let cyclic = sizes[component] > 1 || graph[node].binary_search(&node).is_ok();
        if !cyclic {
            continue;
        }
        composer.report(
            super::super::SCHEMA_INPUT_DEFAULT_CYCLE,
            format!(
                "Input field `{}.{}` has a cyclic default value",
                composer.schema.types[definition.owner].name,
                composer.schema.types[definition.owner].input_fields[definition.field]
                    .name
                    .value
            ),
            definition.span,
        );
        if !composer.can_continue() {
            return;
        }
    }
}
