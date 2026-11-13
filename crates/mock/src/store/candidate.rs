//! Reserve shared map and vector copies before forking candidate state so failure adds no record references.

use super::{CallScope, Store, StoreError, Transaction, work::Work};
use crate::value::ValueBudget;

impl Store {
    /// Share record values and charge work before copying candidate index maps.
    pub fn begin(&self, scope: CallScope) -> Result<Transaction, StoreError> {
        self.begin_with_work(scope, Work::default())
    }

    pub(crate) fn begin_with_work(
        &self,
        scope: CallScope,
        mut work: Work,
    ) -> Result<Transaction, StoreError> {
        let state = &self.current.state;
        work.charge(1 + state.models.len() + state.relations.len() + state.keys.len())?;
        for model in &state.models {
            // Charge copied row and identity-map entries; key and relation indexes remain Arc-shared.
            work.charge(model.records.len() + model.ids.len())?;
        }
        let state = (**state).clone();
        work.charge(0)?;
        Ok(Transaction {
            base: self.current.clone(),
            state,
            scope,
            failed: false,
            values: ValueBudget::with_work(work.clone()),
            work,
        })
    }
}
