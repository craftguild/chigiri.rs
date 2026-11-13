//! Convert the two-way edge operation to an FK patch or an implicit index update, and edit only the same candidate state.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{Record, StoreError, Transaction, keys, relations};
use crate::ir::{FieldId, FieldPlan, ModelId, RelationStorage};
use crate::value::ValueError;

impl Transaction {
    /// Connect both sides and return the resulting relation in selection order.
    pub fn connect(
        &mut self,
        model: ModelId,
        source: &Object,
        relation: &str,
        target: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        self.relate_selected(model, source, relation, target, true)
    }

    /// Select both sides, remove their edges, and return the resulting relation in selection order.
    pub fn disconnect(
        &mut self,
        model: ModelId,
        source: &Object,
        relation: &str,
        target: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        self.relate_selected(model, source, relation, target, false)
    }

    pub(super) fn relation_field(
        &mut self,
        model: ModelId,
        name: &str,
    ) -> Result<(FieldId, ModelId), StoreError> {
        self.work.charge(1 + name.len())?;
        self.base
            .ir
            .relation_target(model, name)
            .ok_or(ValueError("relation field required"))
    }

    pub(super) fn edit_edge(
        &mut self,
        field: FieldId,
        source: u64,
        target: u64,
        connect: bool,
    ) -> Result<(), StoreError> {
        self.writable()?;
        self.work.charge(1)?;
        let ir = self.base.ir.clone();
        let (id, owner, target) = match ir.field_plan(field) {
            Some(FieldPlan::Owner(id)) => (*id, source, target),
            Some(FieldPlan::Inverse { relation, .. }) => (*relation, target, source),
            _ => return Err(ValueError("relation field required")),
        };
        let relation = &ir.relations()[id.0];
        let owner = self.state.models[relation.owner.0]
            .records
            .get(&owner)
            .cloned()
            .ok_or(ValueError("owner record not found"))?;
        let target = self.state.models[relation.target.0]
            .records
            .get(&target)
            .cloned()
            .ok_or(ValueError("target record not found"))?;
        let key = relations::target_key(&relation.storage);
        let reference = keys::record(&ir, key, &target, &mut self.work)?;
        if let RelationStorage::ForeignKey { slots, .. } = &relation.storage {
            let current = keys::references(&ir, key, slots, &owner, &mut self.work)?;
            let list = ir.graph().fields()[relation.field.0].ty.list_depth() != 0;
            let mut changed = current.unwrap_or_default();
            let found = changed
                .iter()
                .any(|value| value.as_ref() == Some(&reference));
            if found == connect {
                return Ok(());
            }
            if connect {
                if !list && !changed.is_empty() {
                    return Err(ValueError("singular relation already connected"));
                }
                self.values.items(changed.len() + 1)?;
                changed.push(Some(reference));
            } else {
                changed.retain(|value| value.as_ref() != Some(&reference));
            }
            let mut patch = Object::new();
            // Reserve the complete component copy before preserving tuple and field order.
            for value in changed.iter().flatten() {
                self.values.scratch(value.cost())?;
            }
            for (position, slot) in slots.iter().enumerate() {
                let values = changed
                    .iter()
                    .map(|key| {
                        key.as_ref()
                            .and_then(|key| key.value(position))
                            .unwrap_or(Value::Null)
                    })
                    .collect::<Vec<_>>();
                let value = if list {
                    Value::List(values)
                } else {
                    values.into_iter().next().unwrap_or(Value::Null)
                };
                patch.insert(ir.slots()[slot.0].name.to_string(), value);
            }
            self.update_row(relation.owner, owner.order, &patch)?;
        } else {
            super::implicit::set(
                &mut self.state,
                id,
                owner.order,
                reference,
                connect,
                &mut self.values,
            )?;
        }
        Ok(())
    }
}
