//! Validate multi-resource reservations atomically and retain soft notices plus the first failure.

use crate::limits::request::{BudgetFailure, KINDS, LimitKind, LimitNotice, policy};
use crate::limits::warning::{LimitOrigin, LimitWarning};
use crate::value::ValueError;
use std::time::Instant;

#[derive(Default)]
pub(super) struct Ledger {
    pub used: [usize; 6],
    pub failure: Option<BudgetFailure>,
    pub started: Option<Instant>,
    warned: [bool; 6],
    pub pending: [Option<LimitWarning>; 6],
    #[cfg(test)]
    pub observer: Option<Box<dyn FnMut(usize) + Send>>,
}

impl Ledger {
    /// Observe elapsed time after completion without adding a hard failure, emitting only a pending soft notice.
    pub fn observe_elapsed_soft(&mut self, now: Instant, origin: &LimitOrigin) {
        let Some(started) = self.started else {
            return;
        };
        if self.failure.is_some() {
            return;
        }
        let kind = LimitKind::ElapsedMs;
        let index = kind as usize;
        let elapsed = now
            .saturating_duration_since(started)
            .as_millis()
            .min(usize::MAX as u128) as usize;
        self.used[index] = self.used[index].max(elapsed);
        if !self.warned[index] && self.used[index] >= policy(kind).soft {
            self.warned[index] = true;
            self.pending[index] = Some(LimitWarning {
                origin: origin.clone(),
                kind,
                observed: self.used[index],
            });
        }
    }

    pub fn fork_startup(&self) -> Self {
        Self {
            used: self.used,
            warned: self.warned,
            ..Self::default()
        }
    }

    pub fn reserve(
        &mut self,
        counts: [usize; 5],
        now: Instant,
        guard: Option<BudgetFailure>,
        origin: &LimitOrigin,
    ) -> Result<(), ValueError> {
        if self.failure.is_some() {
            return Err(ValueError("store work limit"));
        }
        if let Some(started) = self.started {
            let elapsed = now
                .saturating_duration_since(started)
                .as_millis()
                .min(usize::MAX as u128) as usize;
            let index = LimitKind::ElapsedMs as usize;
            self.used[index] = self.used[index].max(elapsed);
        }
        if let Some(failure) = guard {
            self.failure = Some(failure);
            return Err(ValueError("store execution cancelled or expired"));
        }
        if self.started.is_some() && self.used[5] >= policy(LimitKind::ElapsedMs).hard {
            self.failure = Some(BudgetFailure::Deadline);
            return Err(ValueError("request execution expired"));
        }
        let mut next = self.used;
        for (index, count) in counts.into_iter().enumerate() {
            next[index] = next[index].saturating_add(count);
            if next[index] > policy(KINDS[index]).hard {
                self.failure = Some(BudgetFailure::Hard(LimitNotice {
                    kind: KINDS[index],
                    observed: next[index],
                }));
                return Err(ValueError("request aggregate limit"));
            }
        }
        self.used = next;
        for kind in KINDS {
            let index = kind as usize;
            if !self.warned[index] && self.used[index] >= policy(kind).soft {
                self.warned[index] = true;
                self.pending[index] = Some(LimitWarning {
                    origin: origin.clone(),
                    kind,
                    observed: self.used[index],
                });
            }
        }
        Ok(())
    }
}
