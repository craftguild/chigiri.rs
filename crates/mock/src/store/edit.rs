//! Apply primitive writes to candidate state while updating indexes through the same path.

use std::sync::Arc;

use chigiri::core::Object;

use super::{MODEL_RECORDS, Record, StoreError, Transaction, Usage, records, relations};
use crate::ir::ModelId;
use crate::value::ValueError;

impl Transaction {
    /// Create a record while deferring forward-reference relation checks until final validation.
    pub fn create(&mut self, model: ModelId, input: &Object) -> Result<Arc<Record>, StoreError> {
        let result = self.create_inner(model, input);
        self.checked(result)
    }

    fn create_inner(&mut self, model: ModelId, input: &Object) -> Result<Arc<Record>, StoreError> {
        self.writable()?;
        let state = self
            .state
            .models
            .get(model.0)
            .ok_or(ValueError("unknown model"))?;
        if state.records.len() >= MODEL_RECORDS {
            return Err(ValueError("model capacity"));
        }
        let definition = &self.base.ir.models()[model.0];
        self.work.charge(1)?;
        let generated = if definition.auto_id && !input.contains_key("id") {
            Some(super::identity::generate()?)
        } else {
            None
        };
        let order = state.next_order;
        let next_order = order
            .checked_add(1)
            .ok_or(ValueError("registration order exhausted"))?;
        let record = Arc::new(records::build(
            &self.base.ir,
            model,
            input,
            records::Basis {
                order,
                base: None,
                generated_id: generated.as_deref(),
            },
            self.state.usage,
            &mut self.values,
        )?);
        if record
            .id
            .as_ref()
            .is_some_and(|id| state.ids.contains_key(id))
        {
            return Err(ValueError("duplicate model identity"));
        }
        let usage = self.state.usage.replacing(Usage::default(), record.usage)?;
        self.work.charge(record.usage.nodes)?;
        relations::replace(
            &self.base.ir,
            &mut self.state,
            None,
            Some(&record),
            &mut self.work,
        )?;
        let state = &mut self.state.models[model.0];
        state.next_order = next_order;
        if let Some(id) = &record.id {
            state.ids.insert(id.clone(), order);
        }
        state.records.insert(order, record.clone());
        self.state.usage = usage;
        Ok(record)
    }

    pub(super) fn update_row(
        &mut self,
        model: ModelId,
        order: u64,
        patch: &Object,
    ) -> Result<Arc<Record>, StoreError> {
        self.writable()?;
        let old = self
            .state
            .models
            .get(model.0)
            .and_then(|model| model.records.get(&order))
            .cloned()
            .ok_or(ValueError("record not found"))?;
        let record = Arc::new(records::build(
            &self.base.ir,
            model,
            patch,
            records::Basis {
                order: old.order,
                base: Some(&old),
                generated_id: None,
            },
            self.state.usage,
            &mut self.values,
        )?);
        let usage = self.state.usage.replacing(old.usage, record.usage)?;
        self.work.charge(old.usage.nodes + record.usage.nodes)?;
        relations::replace(
            &self.base.ir,
            &mut self.state,
            Some(&old),
            Some(&record),
            &mut self.work,
        )?;
        self.state.models[model.0]
            .records
            .insert(old.order, record.clone());
        self.state.usage = usage;
        Ok(record)
    }
}
