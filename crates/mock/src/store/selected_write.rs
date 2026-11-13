//! Fix each selection before editing, and apply each write or RESTRICT delete atomically.

use std::{collections::BTreeSet, sync::Arc};

use chigiri::core::Object;

use super::{Record, StoreError, Transaction, Usage, keys, query, relations};
use crate::ir::{ModelId, RelationStorage};
use crate::value::ValueError;

impl Transaction {
    /// Apply one bounded patch to every selected record and return copies in selection order.
    pub fn update(
        &mut self,
        model: ModelId,
        options: &Object,
        patch: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        let result = self.update_selection(model, options, patch);
        self.checked(result)
    }

    /// Reject foreign-key references from outside the delete set, then return the deleted count.
    pub fn delete(&mut self, model: ModelId, options: &Object) -> Result<usize, StoreError> {
        let result = self.delete_selection(model, options);
        self.checked(result)
    }

    /// Select both relation sides through a shared query plan and edit them in selection order.
    pub(crate) fn relate_selected(
        &mut self,
        model: ModelId,
        source: &Object,
        relation: &str,
        target: &Object,
        connect: bool,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        let result = self.relate_selection(model, source, relation, target, connect);
        self.checked(result)
    }

    fn select(&mut self, model: ModelId, options: &Object) -> Result<Vec<Arc<Record>>, StoreError> {
        query::find(
            &self.base.ir,
            &self.state,
            model,
            options,
            &mut self.values,
            &mut self.work,
        )
    }

    fn update_selection(
        &mut self,
        model: ModelId,
        options: &Object,
        patch: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        self.writable()?;
        super::patch::validate(&self.base.ir, model, patch, &mut self.values)?;
        let selected = self.select(model, options)?;
        let mut result = Vec::new();
        self.values.container(1)?;
        for row in selected {
            self.values.container(2)?;
            result.push(self.update_row(model, row.order, patch)?);
        }
        Ok(result)
    }

    fn delete_selection(&mut self, model: ModelId, options: &Object) -> Result<usize, StoreError> {
        self.writable()?;
        let selected = self.select(model, options)?;
        self.work.charge(selected.len())?;
        let deleting: BTreeSet<_> = selected.iter().map(|row| row.order).collect();
        for row in &selected {
            for (index, relation) in self.base.ir.relations().iter().enumerate() {
                self.work.charge(1)?;
                let RelationStorage::ForeignKey { key, .. } = relation.storage else {
                    continue;
                };
                if relation.target != model {
                    continue;
                }
                let key = keys::record(&self.base.ir, key, row, &mut self.work)?;
                if let Some(owners) = self.state.relations[index].get(&key) {
                    self.work.charge(owners.len())?;
                    if owners
                        .keys()
                        .any(|owner| relation.owner != model || !deleting.contains(owner))
                    {
                        return Err(ValueError("referenced record outside deletion set"));
                    }
                }
            }
        }
        for row in &selected {
            self.work.charge(row.usage.nodes)?;
            relations::replace(
                &self.base.ir,
                &mut self.state,
                Some(row),
                None,
                &mut self.work,
            )?;
            let state = &mut self.state.models[model.0];
            if let Some(id) = &row.id {
                state.ids.remove(id);
            }
            state.records.remove(&row.order);
            self.state.usage = self.state.usage.replacing(row.usage, Usage::default())?;
        }
        Ok(selected.len())
    }

    fn relate_selection(
        &mut self,
        model: ModelId,
        source: &Object,
        relation: &str,
        target: &Object,
        connect: bool,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        self.writable()?;
        let (field, target_model) = self.relation_field(model, relation)?;
        let sources = self.select(model, source)?;
        let targets = self.select(target_model, target)?;
        self.work.charge(
            sources
                .len()
                .checked_mul(targets.len())
                .ok_or(ValueError("relation selection work limit"))?,
        )?;
        for source in &sources {
            for target in &targets {
                self.edit_edge(field, source.order, target.order, connect)?;
            }
        }
        // Empty selections return no relations but still honor copy reservations.
        self.values.container(1)?;
        sources
            .iter()
            .map(|row| {
                self.values.container(2)?;
                self.state.models[model.0]
                    .records
                    .get(&row.order)
                    .cloned()
                    .ok_or(ValueError("selected source record missing"))
            })
            .collect()
    }
}
