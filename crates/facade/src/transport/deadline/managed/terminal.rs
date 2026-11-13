//! Managed request admission and termination metadata, without application owners.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use chigiri_http::ResponseCompletionOutcome;

#[derive(Default)]
pub(super) struct Terminal {
    selected: AtomicU8,
    fault: AtomicBool,
    poisoned: AtomicBool,
    finite_admitted: AtomicBool,
}

impl Terminal {
    pub(super) fn finite_admitted(&self) -> bool {
        self.finite_admitted.load(Ordering::Acquire)
    }

    pub(super) fn admit_finite(&self) {
        self.finite_admitted.store(true, Ordering::Release);
    }

    pub(super) fn poisoned_flag(&self) -> &AtomicBool {
        &self.poisoned
    }
    pub(super) fn poison(&self) {
        self.fail();
        self.poisoned.store(true, Ordering::Release);
    }
    pub(super) fn select(&self, timed_out: bool) {
        let _ = self.selected.compare_exchange(
            0,
            if timed_out { 2 } else { 1 },
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub(super) fn outcome(&self, timed_out: bool) -> ResponseCompletionOutcome {
        self.select(timed_out);
        match self.selected.load(Ordering::Acquire) {
            2 => ResponseCompletionOutcome::TimedOut,
            _ => ResponseCompletionOutcome::Failed,
        }
    }

    pub(super) fn fail(&self) {
        self.select(false);
        self.fault.store(true, Ordering::Release);
    }

    pub(super) fn faulted(&self) -> bool {
        self.fault.load(Ordering::Acquire)
    }
}
