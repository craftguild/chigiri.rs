//! Separates the request's starting generation from its private candidate and
//! swaps the candidate once after all roots are prepared.

use super::{Snapshot, Store, StoreError, relations};
use crate::value::{ValueBudget, ValueError};

/// Store owned by one request. Internal root changes are not visible to other
/// requests.
pub struct RequestCandidate {
    base: Snapshot,
    store: Store,
    values: ValueBudget,
}

/// Closed candidate after all jobs finish. Its base remains the generation seen
/// at request start.
pub struct RequestPrepared {
    base: Snapshot,
    result: Snapshot,
    values: ValueBudget,
}

impl RequestPrepared {
    /// Check only the shared ledger immediately before the swap.
    pub fn check(&self) -> Result<(), StoreError> {
        self.values.work.check()
    }
}

impl Store {
    /// Create a private candidate from the snapshot observed after acquiring the
    /// mutation permit.
    pub fn begin_request(&self, values: ValueBudget) -> RequestCandidate {
        RequestCandidate {
            base: self.snapshot(),
            store: Store {
                current: self.snapshot(),
            },
            values,
        }
    }

    /// Swap the candidate inside the cancellation fence and return the old
    /// snapshot so it can be dropped outside the fence.
    /// The caller records terminal state in the same fence and retains the
    /// candidate and return value until after releasing the lock. Ownership of a
    /// failed candidate stays with the caller so its potentially expensive drop
    /// also happens outside the fence.
    pub fn commit_request(&mut self, prepared: &RequestPrepared) -> Result<Snapshot, StoreError> {
        self.check_request(prepared)?;
        Ok(std::mem::replace(
            &mut self.current,
            prepared.result.clone(),
        ))
    }

    /// Check generation and ledger without changing store or publication ordering.
    pub(crate) fn check_request(&self, prepared: &RequestPrepared) -> Result<(), StoreError> {
        prepared.check()?;
        if !self.current.same_version(&prepared.base) {
            return Err(ValueError("stale request transaction"));
        }
        Ok(())
    }
}

impl RequestCandidate {
    /// Access the private store through the existing per-root transaction path.
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Candidate generation read by later roots and hooks. Existing child results
    /// retain their own snapshots.
    pub fn snapshot(&self) -> Snapshot {
        self.store.snapshot()
    }

    /// Prepare the candidate after the caller closes admission and all jobs finish.
    pub fn prepare(mut self) -> Result<RequestPrepared, StoreError> {
        self.values.work.charge(0)?;
        relations::validate(
            &self.base.ir,
            &self.store.current.state,
            &mut self.values.work,
        )?;
        Ok(RequestPrepared {
            base: self.base,
            result: self.store.current,
            values: self.values,
        })
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
