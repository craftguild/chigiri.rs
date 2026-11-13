//! Apply a prevalidated query plan to one snapshot or candidate, separating page collection from total counts.

use super::query_plan::QueryPlan;
use super::work::Work;
use super::{Record, Snapshot, State, StoreError, Transaction};
use crate::ir::{MockIr, ModelId};
use crate::value::{ValueBudget, ValueError};
use chigiri::core::Object;
use std::sync::Arc;

impl Snapshot {
    /// Apply closed predicates, sorting, and pagination to a snapshot.
    pub fn find(&self, model: ModelId, options: &Object) -> Result<Vec<Arc<Record>>, StoreError> {
        let mut work = Work::default();
        find(
            &self.ir,
            &self.state,
            model,
            options,
            &mut ValueBudget::with_work(work.clone()),
            &mut work,
        )
    }

    /// Count matching snapshot records without applying page limits or copying records.
    pub fn count(&self, model: ModelId, options: &Object) -> Result<usize, StoreError> {
        let mut work = Work::default();
        count(
            &self.ir,
            &self.state,
            model,
            options,
            &mut ValueBudget::with_work(work.clone()),
            &mut work,
        )
    }
}

impl Transaction {
    /// Apply closed predicates, sorting, and pagination to candidate state.
    pub fn find(
        &mut self,
        model: ModelId,
        options: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        let result = if self.failed {
            Err(ValueError("host failure prevents execution"))
        } else {
            find(
                &self.base.ir,
                &self.state,
                model,
                options,
                &mut self.values,
                &mut self.work,
            )
        };
        self.checked(result)
    }

    /// Count matching candidate records under the transaction's shared budget.
    pub fn count(&mut self, model: ModelId, options: &Object) -> Result<usize, StoreError> {
        let result = if self.failed {
            Err(ValueError("host failure prevents execution"))
        } else {
            count(
                &self.base.ir,
                &self.state,
                model,
                options,
                &mut self.values,
                &mut self.work,
            )
        };
        self.checked(result)
    }
}

pub(super) fn find(
    ir: &MockIr,
    state: &State,
    model: ModelId,
    options: &Object,
    budget: &mut ValueBudget,
    work: &mut Work,
) -> Result<Vec<Arc<Record>>, StoreError> {
    let plan = QueryPlan::build(ir, model, options, false, budget, work)?;
    budget.container(1)?;
    if plan.limit == Some(0) {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for record in state.models[model.0].records.values() {
        if plan.matches(record, work)? {
            rows.push(record);
        }
    }
    plan.sort(&mut rows, work)?;
    let rows = rows
        .into_iter()
        .skip(plan.offset)
        .take(plan.limit.unwrap_or(usize::MAX));
    let length = rows.size_hint().0;
    budget.items(length)?;
    let mut result = Vec::new();
    for row in rows {
        budget.container(2)?;
        work.charge(1)?;
        result.push(row.clone());
    }
    Ok(result)
}

pub(super) fn count(
    ir: &MockIr,
    state: &State,
    model: ModelId,
    options: &Object,
    budget: &mut ValueBudget,
    work: &mut Work,
) -> Result<usize, StoreError> {
    let plan = QueryPlan::build(ir, model, options, true, budget, work)?;
    let mut count = 0usize;
    for record in state.models[model.0].records.values() {
        if plan.matches(record, work)? {
            count = count.checked_add(1).ok_or(ValueError("count overflow"))?;
        }
    }
    Ok(count)
}
