//! Separate external identity from internal record position while keeping returned handles bound to one snapshot.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{Record, Snapshot, StoreError, identity};
use crate::ir::ModelId;
use crate::value::{ValueBudget, equal};

impl Snapshot {
    pub(crate) fn row(&self, model: ModelId, row: u64) -> Option<&Arc<Record>> {
        self.state.models.get(model.0)?.records.get(&row)
    }

    pub(crate) fn matching_record(
        &self,
        model: ModelId,
        object: &Object,
        budget: &mut ValueBudget,
    ) -> Result<Option<Arc<Record>>, StoreError> {
        let definition = &self.ir.models()[model.0];
        let id = identity::value(
            definition
                .key
                .and_then(|key| object.get(&self.ir.graph().fields()[key.0].name)),
        )?;
        let state = &self.state.models[model.0];
        let matches = |record: &Record, budget: &mut ValueBudget| -> Result<bool, StoreError> {
            budget
                .work
                .charge(record.usage.nodes + record.usage.bytes)?;
            Ok(record.values().all(|(slot, value)| {
                equal(
                    value,
                    object
                        .get(&self.ir.slots()[slot.0].name)
                        .unwrap_or(&Value::Null),
                )
            }))
        };
        if let Some(id) = id {
            return match self.record(model, &id) {
                Some(record) if matches(record, budget)? => Ok(Some(record.clone())),
                _ => Ok(None),
            };
        }
        for record in state.records.values() {
            if record.id.is_none() && matches(record, budget)? {
                return Ok(Some(record.clone()));
            }
        }
        Ok(None)
    }
}
