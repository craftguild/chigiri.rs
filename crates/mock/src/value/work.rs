//! Charge value, host, regex, and elapsed work to one request-scoped limit ledger.

mod ledger;
#[cfg(test)]
mod limit_tests;
#[cfg(test)]
mod origin_tests;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use super::ValueError;
use crate::cancellation::Cancellation;
#[cfg(test)]
use crate::limits::request::policy;
use crate::limits::request::{BudgetFailure, LimitKind, LimitNotice};
use crate::limits::warning::{LimitOrigin, LimitWarning};
use ledger::Ledger;

#[cfg(test)]
pub(crate) const MAX_WORK: usize = crate::api::WORK_LIMIT;
use crate::api::constraints::REGEX_WORK_FACTOR;
#[cfg(test)]
use crate::api::constraints::{REGEX_MATCH_BYTES as MATCH_BYTES, REGEX_MATCHES as MATCHES};

/// Cloneable handle to one shared work ledger and warning sink.
#[derive(Clone)]
pub(crate) struct Work {
    ledger: Arc<Mutex<Ledger>>,
    warnings: Arc<OnceLock<WarningSink>>,
    origin: LimitOrigin,
    guard: Option<(Cancellation, Cancellation, Instant)>,
}
type WarningSink = Box<dyn Fn(LimitWarning) + Send + Sync>;

impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Work").finish_non_exhaustive()
    }
}

impl Default for Work {
    fn default() -> Self {
        Self {
            ledger: Arc::new(Mutex::new(Ledger::default())),
            warnings: Arc::default(),
            origin: LimitOrigin::default(),
            guard: None,
        }
    }
}

impl Work {
    /// Share accounting and warnings while assigning a new diagnostic origin to this handle.
    pub fn at(&self, origin: LimitOrigin) -> Self {
        let mut work = self.clone();
        work.origin = origin;
        work
    }

    /// Fork startup accounting for an independent seed-validation pass.
    pub fn fork_startup(&self) -> Result<Self, ValueError> {
        let ledger = self
            .ledger
            .lock()
            .map_err(|_| ValueError("work ledger poisoned"))?;
        if ledger.failure.is_some() {
            return Err(ValueError("startup work already failed"));
        }
        Ok(Self {
            ledger: Arc::new(Mutex::new(ledger.fork_startup())),
            warnings: Arc::default(),
            origin: LimitOrigin::default(),
            guard: None,
        })
    }

    /// Record the request start once so every phase uses the same elapsed-time origin.
    pub fn start_request(&self, started: Instant) {
        self.ledger
            .lock()
            .expect("request ledger lock")
            .started
            .get_or_insert(started);
    }

    /// Return the first failure shared across all request phases.
    pub fn failure(&self) -> Option<BudgetFailure> {
        self.ledger
            .lock()
            .map(|ledger| ledger.failure)
            .unwrap_or(Some(BudgetFailure::Execution))
    }

    /// Drain pending notices so each threshold is reported at most once.
    pub fn take_warnings(&self) -> Vec<LimitNotice> {
        let mut ledger = self.ledger.lock().expect("request ledger lock");
        ledger
            .pending
            .iter_mut()
            .filter_map(Option::take)
            .map(|warning| warning.notice())
            .collect()
    }

    /// Install the request warning sink before work begins.
    /// Delivery occurs outside the ledger lock so the sink may perform independent I/O.
    pub fn with_warnings(&self, sink: impl Fn(LimitWarning) + Send + Sync + 'static) {
        assert!(
            self.warnings.set(Box::new(sink)).is_ok(),
            "the warning sink may be set only once"
        );
    }

    /// Observe elapsed soft limits without introducing a hard or cancellation failure after commit.
    pub fn observe_elapsed_soft(&self) {
        let mut ledger = self.ledger.lock().expect("request ledger lock");
        ledger.observe_elapsed_soft(Instant::now(), &self.origin);
        let pending = self
            .warnings
            .get()
            .map(|_| std::mem::take(&mut ledger.pending));
        drop(ledger);
        if let (Some(sink), Some(pending)) = (self.warnings.get(), pending) {
            for notice in pending.into_iter().flatten() {
                sink(notice);
            }
        }
    }

    #[cfg(test)]
    pub fn observe(&mut self, observer: impl FnMut(usize) + Send + 'static) {
        self.ledger.lock().unwrap().observer = Some(Box::new(observer));
    }

    #[cfg(test)]
    pub fn match_count(&self) -> usize {
        self.ledger.lock().unwrap().used[LimitKind::RegexMatches as usize]
    }

    pub fn guard(&mut self, cancel: &Cancellation, shutdown: &Cancellation, deadline: Instant) {
        self.guard = Some((cancel.clone(), shutdown.clone(), deadline));
    }

    pub fn charge(&mut self, count: usize) -> Result<(), ValueError> {
        self.reserve([count, 0, 0, 0, 0], Instant::now())
    }

    fn reserve(&mut self, counts: [usize; 5], now: Instant) -> Result<(), ValueError> {
        // Check cancellation before taking the ledger lock to preserve lock ordering.
        let failure = self
            .guard
            .as_ref()
            .and_then(|(cancel, shutdown, deadline)| {
                if cancel.cancelled() || shutdown.cancelled() {
                    Some(BudgetFailure::Cancelled)
                } else if now >= *deadline {
                    Some(BudgetFailure::Deadline)
                } else {
                    None
                }
            });
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| ValueError("work ledger poisoned"))?;
        #[cfg(test)]
        if let Some(observer) = &mut ledger.observer {
            observer(counts[0]);
        }
        // Refresh time after acquiring the ledger without reacquiring cancellation state.
        let now = now.max(Instant::now());
        let failure = failure.or_else(|| {
            self.guard
                .as_ref()
                .and_then(|(_, _, deadline)| (now >= *deadline).then_some(BudgetFailure::Deadline))
        });
        let result = ledger.reserve(counts, now, failure, &self.origin);
        let pending = self
            .warnings
            .get()
            .map(|_| std::mem::take(&mut ledger.pending));
        drop(ledger);
        if let (Some(sink), Some(pending)) = (self.warnings.get(), pending) {
            for notice in pending.into_iter().flatten() {
                sink(notice);
            }
        }
        result
    }

    /// Check the shared failure state without changing any reservation.
    pub fn check(&self) -> Result<(), ValueError> {
        if self.failure().is_some() {
            Err(ValueError("store work limit"))
        } else {
            Ok(())
        }
    }

    pub fn reserve_match(&mut self, bytes: usize) -> Result<(), ValueError> {
        if bytes > super::MAX_STRING_BYTES {
            return self.charge(usize::MAX);
        }
        self.reserve(
            [1 + REGEX_WORK_FACTOR * (bytes + 1), 0, 0, 1, bytes],
            Instant::now(),
        )
    }

    /// Validate all resource deltas atomically; a hard-limit failure records no partial usage or warning.
    pub fn reserve_value(&mut self, nodes: usize, bytes: usize) -> Result<(), ValueError> {
        self.reserve(
            [bytes.saturating_add(1), nodes, bytes, 0, 0],
            Instant::now(),
        )
    }

    /// Prefer a proven execution failure over cleanup cancellation while preserving earlier hard failures.
    pub fn fail_independent(&mut self) {
        if let Ok(mut ledger) = self.ledger.lock()
            && matches!(ledger.failure, None | Some(BudgetFailure::Cancelled))
        {
            ledger.failure = Some(BudgetFailure::Execution);
        }
    }

    /// Prefer a local hard-limit failure over cleanup cancellation while preserving earlier failures.
    pub fn fail_local(&mut self, limit: crate::limits::request::LocalLimit) {
        if let Ok(mut ledger) = self.ledger.lock()
            && matches!(ledger.failure, None | Some(BudgetFailure::Cancelled))
        {
            ledger.failure = Some(BudgetFailure::LocalHard(limit));
        }
    }

    pub fn fail(&mut self) {
        self.fail_with(BudgetFailure::Execution);
    }

    pub fn fail_limit(&mut self, kind: LimitKind, observed: usize) {
        self.fail_with(BudgetFailure::Hard(LimitNotice { kind, observed }));
    }

    pub fn fail_with(&mut self, failure: BudgetFailure) {
        if let Ok(mut ledger) = self.ledger.lock() {
            ledger.failure.get_or_insert(failure);
        }
    }
}

#[cfg(test)]
mod observation_tests {
    use super::*;

    #[test]
    fn before_and_after_handles_share_remaining_capacity_and_sticky_failure() {
        let mut business = Work::default();
        business.charge(8).unwrap();
        let mut after = business.clone();
        after.charge(MAX_WORK - 8).unwrap();
        assert!(business.check().is_ok());
        assert!(after.charge(1).is_err());
        assert!(business.check().is_err());
    }

    #[test]
    fn regex_and_value_work_accumulate_across_phases() {
        let mut business = Work::default();
        let mut after = business.clone();
        business
            .reserve_value(super::super::MAX_NODES - 1, 0)
            .unwrap();
        after.reserve_value(1, 0).unwrap();
        assert!(after.reserve_value(1, 0).is_err());
        assert!(business.check().is_err());
        let mut business = Work::default();
        let mut after = business.clone();
        for _ in 0..MATCHES - 1 {
            business.reserve_match(0).unwrap();
        }
        after.reserve_match(0).unwrap();
        assert!(after.reserve_match(0).is_err());
        assert!(business.check().is_err());
        assert_eq!(
            policy(LimitKind::LogicalBytes).hard,
            super::super::MAX_BYTES
        );
        assert_eq!(policy(LimitKind::RegexInputBytes).hard, MATCH_BYTES);
    }
}
