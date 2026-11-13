//! Reconstruct one canonical publication against a single fresh event snapshot.

mod object;
#[cfg(test)]
mod tests;
mod value;

use crate::ir::FieldId;
use crate::output::OutputValue;
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError};

use super::CapturedPublication;

impl CapturedPublication {
    pub(super) fn project(
        &self,
        snapshot: Snapshot,
        field: FieldId,
        budget: &mut ValueBudget,
    ) -> Result<OutputValue, ValueError> {
        let graph = snapshot.ir().graph();
        let definition = graph
            .fields()
            .get(field.0)
            .ok_or(ValueError("subscription field required"))?;
        let parent = &graph.types()[definition.parent.0].name;
        let coordinate = self
            .coordinates()
            .find(|coordinate| same_coordinate(coordinate, parent, &definition.name))
            .ok_or(ValueError("publication coordinate mismatch"))?;
        root(coordinate, self.payload(), snapshot, field, budget)
    }
}

pub(super) fn root(
    coordinate: &str,
    payload: &super::capture::CapturedValue,
    snapshot: Snapshot,
    field: FieldId,
    budget: &mut ValueBudget,
) -> Result<OutputValue, ValueError> {
    let ir = snapshot.ir();
    let graph = ir.graph();
    let subscription = graph
        .subscription()
        .ok_or(ValueError("subscription root required"))?;
    let definition = graph
        .fields()
        .get(field.0)
        .filter(|definition| definition.parent == subscription)
        .ok_or(ValueError("subscription field required"))?;
    let parent = &graph.types()[subscription.0].name;
    if !same_coordinate(coordinate, parent, &definition.name) {
        return Err(ValueError("publication coordinate mismatch"));
    }
    value::typed(payload, &snapshot, &definition.ty, budget, 1)
}

pub(super) fn same_coordinate(actual: &str, parent: &str, field: &str) -> bool {
    actual.len() == parent.len() + 1 + field.len()
        && actual.as_bytes().get(parent.len()) == Some(&b'.')
        && actual.get(..parent.len()) == Some(parent)
        && actual.get(parent.len() + 1..) == Some(field)
}
