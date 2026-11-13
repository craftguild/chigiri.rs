//! Record one terminal request outcome and distinguish response completion from cancellation.

use std::sync::atomic::Ordering;

use super::{completion::Failure, session::Session};
use crate::limits::request::{BudgetFailure, LimitNotice, policy};

/// Structured, sanitized diagnostic for request termination.
#[derive(serde::Serialize)]
pub struct TerminalDiagnostic {
    /// Mock-specific request correlation ID.
    pub request_id: u64,
    /// Response status, absent when cancellation prevented response creation.
    pub status: Option<u16>,
    /// Stable transaction state: `committed`, `rolled_back`, or `not_started`.
    pub transaction: &'static str,
    /// Stable terminal cause; a rejection remains distinct even when its HTTP status is 200.
    pub cause: &'static str,
    /// Severity corresponding to success, internal failure, or explicit rejection.
    pub level: &'static str,
    /// Boundary that observed termination, such as response completion or drop.
    pub phase: &'static str,
    /// Shared or local hard-limit details, when applicable.
    pub limit: Option<LimitDiagnostic>,
    /// Whether per-failure diagnostics exceeded their fixed count limit.
    pub diagnostics_truncated: bool,
    /// Enabled after-hooks that were dropped before execution reached them.
    pub unreached_after_hooks: Vec<crate::hooks::HookSlot>,
}

/// Resource name, configured thresholds, and observed usage for a terminal limit failure.
#[derive(serde::Serialize)]
pub struct LimitDiagnostic {
    name: &'static str,
    soft: Option<usize>,
    hard: usize,
    observed: Option<usize>,
}

impl Session {
    pub(super) fn report_terminal(&self, status: Option<u16>, phase: &'static str) {
        if self.trace.terminal_reported.swap(true, Ordering::AcqRel) {
            return;
        }
        let committed = self
            .client
            .get()
            .is_some_and(|client| client.request_committed());
        if !committed {
            self.observe_execution_failure();
        }
        let transaction = if committed {
            "committed"
        } else if self.client.get().is_some() {
            "rolled_back"
        } else {
            "not_started"
        };
        let failure = self.failure.lock().expect("Request failed").clone();
        let rejections = self
            .client
            .get()
            .and_then(crate::service::Client::rejections);
        let started = self.started.load(Ordering::Acquire);
        let trace = self.trace.clone();
        // Mark terminal delivery first so the scope fallback cannot redeliver during teardown.
        // Retain only diagnostic data, never the session, client, or request, until the final job completes.
        let terminal = move || {
            let (cause, level, limit) = if committed {
                ("success", "info", None)
            } else {
                let failure =
                    Failure::include_rejection(failure, rejections.and_then(|state| state.get()));
                outcome(failure, trace.execution_budget.failure(), started)
            };
            (trace.terminal)(TerminalDiagnostic {
                request_id: trace.id,
                status,
                transaction,
                cause,
                level,
                phase,
                limit,
                diagnostics_truncated: trace.diagnostics_truncated(),
                unreached_after_hooks: trace.unreached_after_hooks(),
            });
        };
        match self.client.get() {
            Some(client) => client.terminal_after_jobs(terminal),
            None => terminal(),
        }
    }
}

fn outcome(
    mut failure: Option<Failure>,
    budget: Option<BudgetFailure>,
    started: bool,
) -> (&'static str, &'static str, Option<LimitDiagnostic>) {
    if matches!(
        failure,
        None | Some(Failure::Rejected(..) | Failure::Business(_))
    ) {
        failure = match budget {
            Some(BudgetFailure::Hard(notice)) => Some(Failure::Limit(notice)),
            Some(BudgetFailure::LocalHard(limit)) => Some(Failure::LocalLimit(limit)),
            Some(BudgetFailure::Deadline) => Some(Failure::Timeout),
            Some(BudgetFailure::Execution) => Some(Failure::Internal),
            _ => failure,
        };
    }
    match failure {
        Some(Failure::Rejected(..) | Failure::Business(_)) => ("rejected", "notice", None),
        Some(Failure::Limit(notice)) => ("hard_limit", "error", Some(detail(notice))),
        Some(Failure::LocalLimit(limit)) => (
            "hard_limit",
            "error",
            Some(LimitDiagnostic {
                name: limit.name(),
                soft: None,
                hard: limit.hard(),
                observed: limit.observed(),
            }),
        ),
        Some(Failure::Timeout) => ("timeout", "error", None),
        Some(Failure::OutputLimit) => ("output_limit", "error", None),
        Some(Failure::Internal | Failure::SourceStopped) => ("execution_failed", "error", None),
        None if started => ("cancelled", "error", None),
        None => ("not_started", "info", None),
    }
}

fn detail(notice: LimitNotice) -> LimitDiagnostic {
    let policy = policy(notice.kind);
    LimitDiagnostic {
        name: policy.name,
        soft: Some(policy.soft),
        hard: policy.hard,
        observed: Some(notice.observed),
    }
}
