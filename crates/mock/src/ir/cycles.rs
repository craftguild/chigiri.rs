//! Find required model-reference cycles that cannot be satisfied by finite mock values.

use super::{FieldId, FieldPlan, MockIr, ModelId};

/// Format a dependency cycle using model names and GraphQL field coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequiredReferenceCycle {
    path: Vec<String>,
}

impl RequiredReferenceCycle {
    /// GraphQL field coordinates in cycle order, such as `A.b`, `B.a`, and `A`.
    pub fn path(&self) -> &[String] {
        &self.path
    }
}

impl MockIr {
    /// Return required singular-relation cycles with no nullable, list, or manual-field escape.
    pub fn required_reference_cycles(&self) -> Vec<RequiredReferenceCycle> {
        let edges = edges(self);
        components(&edges)
            .into_iter()
            .filter_map(|component| cycle(self, &edges, &component))
            .collect()
    }
}

type Edge = (ModelId, FieldId);

fn edges(ir: &MockIr) -> Vec<Vec<Edge>> {
    let mut result = vec![Vec::new(); ir.models().len()];
    for (index, field) in ir.graph().fields().iter().enumerate() {
        let id = FieldId(index);
        if field.ty.nullable() || field.ty.list_depth() != 0 {
            continue;
        }
        let (Some(owner), Some(target)) =
            (ir.model_for(field.parent), ir.model_for(field.ty.named))
        else {
            continue;
        };
        if matches!(ir.field_plan(id), Some(FieldPlan::Stored(_))) {
            result[owner.0].push((target, id));
        }
    }
    result
}

fn components(edges: &[Vec<Edge>]) -> Vec<Vec<ModelId>> {
    let mut order = Vec::new();
    let mut seen = vec![false; edges.len()];
    for node in 0..edges.len() {
        finish(ModelId(node), edges, &mut seen, &mut order);
    }
    let mut reverse = vec![Vec::new(); edges.len()];
    for (owner, targets) in edges.iter().enumerate() {
        for (target, _) in targets {
            reverse[target.0].push(ModelId(owner));
        }
    }
    seen.fill(false);
    let mut result = Vec::new();
    for node in order.into_iter().rev() {
        if seen[node.0] {
            continue;
        }
        let mut component = Vec::new();
        collect(node, &reverse, &mut seen, &mut component);
        component.sort();
        result.push(component);
    }
    result.sort_by_key(|component| component[0]);
    result
}

fn finish(node: ModelId, edges: &[Vec<Edge>], seen: &mut [bool], order: &mut Vec<ModelId>) {
    if std::mem::replace(&mut seen[node.0], true) {
        return;
    }
    for (target, _) in &edges[node.0] {
        finish(*target, edges, seen, order);
    }
    order.push(node);
}

fn collect(node: ModelId, reverse: &[Vec<ModelId>], seen: &mut [bool], out: &mut Vec<ModelId>) {
    if std::mem::replace(&mut seen[node.0], true) {
        return;
    }
    out.push(node);
    for owner in &reverse[node.0] {
        collect(*owner, reverse, seen, out);
    }
}

fn cycle(
    ir: &MockIr,
    edges: &[Vec<Edge>],
    component: &[ModelId],
) -> Option<RequiredReferenceCycle> {
    let member = |model: ModelId| component.binary_search(&model).is_ok();
    if component.len() == 1
        && !edges[component[0].0]
            .iter()
            .any(|edge| edge.0 == component[0])
    {
        return None;
    }
    let start = component[0];
    let mut visited = vec![false; edges.len()];
    let mut fields = Vec::new();
    find(start, start, edges, &member, &mut visited, &mut fields)?;
    let mut path = fields
        .iter()
        .map(|field| {
            let field = &ir.graph().fields()[field.0];
            format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name)
        })
        .collect::<Vec<_>>();
    let last = &ir.graph().fields()[fields.last()?.0];
    path.push(ir.graph().types()[last.ty.named.0].name.to_string());
    Some(RequiredReferenceCycle { path })
}

fn find(
    node: ModelId,
    start: ModelId,
    edges: &[Vec<Edge>],
    member: &impl Fn(ModelId) -> bool,
    visited: &mut [bool],
    fields: &mut Vec<FieldId>,
) -> Option<()> {
    visited[node.0] = true;
    for (target, field) in &edges[node.0] {
        if !member(*target) {
            continue;
        }
        fields.push(*field);
        if *target == start
            || (!visited[target.0]
                && find(*target, start, edges, member, visited, fields).is_some())
        {
            return Some(());
        }
        fields.pop();
    }
    None
}

#[cfg(test)]
mod tests {
    use chigiri::core::Source;

    use super::MockIr;

    fn cycles(source: &str) -> Vec<Vec<String>> {
        MockIr::parse(vec![Source::new("schema.graphql", source)])
            .unwrap()
            .required_reference_cycles()
            .into_iter()
            .map(|cycle| cycle.path().to_vec())
            .collect()
    }

    #[test]
    fn required_singular_model_cycles_are_deterministic() {
        let source = "type A{id:ID! b:B!} type B{id:ID! a:A!} type Query{a:A}";
        assert_eq!(
            cycles(source),
            vec![vec!["A.b".to_string(), "B.a".to_string(), "A".to_string()]]
        );
        assert_eq!(
            cycles("type Node{id:ID! next:Node!} type Query{node:Node}"),
            vec![vec!["Node.next".to_string(), "Node".to_string()]]
        );
    }

    #[test]
    fn nullable_lists_relations_and_acyclic_values_break_required_cycles() {
        for source in [
            "type A{id:ID! b:B} type B{id:ID! a:A!} type Query{a:A}",
            "type A{id:ID! bs:[B!]!} type B{id:ID! a:A!} type Query{a:A}",
            "type A{id:ID! bs:[B!]! @relation} type B{id:ID! as:[A!]! @relation} type Query{a:A}",
            "type A{id:ID! bId:ID! b:B! @relation(fields:[\"bId\"],references:[\"id\"],name:\"Pair\")} type B{id:ID! a:A! @relation(name:\"Pair\")} type Query{a:A}",
            "type A{id:ID! b:B!} type B{id:ID! value:Int!} type Query{a:A}",
        ] {
            assert!(cycles(source).is_empty(), "{source}");
        }
    }
}
