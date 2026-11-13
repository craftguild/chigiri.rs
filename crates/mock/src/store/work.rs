//! Charge store operations to the same shared work ledger as value validation.

use crate::cancellation::Cancellation;
#[cfg(test)]
pub(super) use crate::value::work::MAX_WORK;
pub(super) use crate::value::work::Work;
use std::time::Instant;

impl super::Transaction {
    pub(crate) fn value_budget(&mut self) -> &mut crate::value::ValueBudget {
        &mut self.values
    }

    pub(crate) fn execution_guard(
        &mut self,
        cancel: &Cancellation,
        shutdown: &Cancellation,
        deadline: Instant,
    ) {
        self.work.guard(cancel, shutdown, deadline);
        self.values.share_work(self.work.clone());
    }
}
