//! Deliver each worker and receiver failure once while bounding independent diagnostic sites.

use super::{HookEvent, ResolverDiagnostic, http::Trace, session::Location};
use crate::hooks::{HookSlot, HookTiming};
use crate::ir::FieldId;
use crate::limits::request::{BudgetFailure, LimitKind};
use crate::luau::ScriptError;

#[derive(PartialEq, Eq)]
enum Owner {
    Hook(HookSlot),
    Resolver(FieldId),
}

#[derive(PartialEq, Eq)]
enum Key {
    Site(Owner, &'static str, (usize, usize), &'static str),
    Limit(LimitKind),
    Local(crate::limits::request::LocalLimit),
    Deadline,
}

#[derive(Default)]
pub(super) struct Seen {
    keys: Vec<Key>,
    pub truncated: bool,
}

impl Trace {
    fn claim_diagnostic(&self, owner: Owner, error: &ScriptError) -> bool {
        // Cleanup cancellation does not add another diagnostic after the original cause.
        if error.cancelled {
            return false;
        }
        let key = if matches!(error.phase, "budget" | "limit") {
            match self.execution_budget.failure() {
                Some(BudgetFailure::Hard(notice)) => Key::Limit(notice.kind),
                Some(BudgetFailure::LocalHard(limit)) => Key::Local(limit),
                Some(BudgetFailure::Deadline) => Key::Deadline,
                _ => Key::Site(owner, error.phase, error.body, error.message),
            }
        } else {
            Key::Site(owner, error.phase, error.body, error.message)
        };
        let mut seen = self
            .seen_diagnostics
            .lock()
            .expect("diagnostic deduplication lock");
        if seen.keys.contains(&key) {
            return false;
        }
        // Bound the number of distinct diagnostic keys retained for one request.
        if seen.keys.len() == crate::limits::QUEUED_JOBS {
            seen.truncated = true;
            return false;
        }
        seen.keys.push(key);
        true
    }

    pub(super) fn hook_diagnostic(
        &self,
        slot: HookSlot,
        location: &Location,
        mut error: ScriptError,
    ) {
        if slot.timing() == HookTiming::After && !error.cancelled {
            self.stop_observations();
        }
        if !self.claim_diagnostic(Owner::Hook(slot), &error) {
            return;
        }
        error.hook = Some(slot);
        (self.events)(HookEvent {
            request_id: self.id,
            slot,
            module: Some(slot.path().to_owned()),
            coordinate: location.coordinate.clone(),
            path: location.path.clone(),
            record: Err(error),
        });
    }

    pub(super) fn resolver_diagnostic(
        &self,
        field: FieldId,
        coordinate: String,
        module: Option<String>,
        error: ScriptError,
    ) {
        if !self.claim_diagnostic(Owner::Resolver(field), &error) {
            return;
        }
        let phase = error.phase;
        let cause = error.cause;
        let position = error.source_position();
        (self.diagnostics)(ResolverDiagnostic {
            stack: error.stack.clone(),
            request_id: self.id,
            coordinate,
            module,
            phase,
            cause,
            member: error.member.clone(),
            expected: nonempty(error.expected.clone()),
            actual: nonempty(error.actual.clone()),
            hint: nonempty(error.hint.clone()),
            line: position.map(|position| position.0),
            column: position.map(|position| position.1),
            cancelled: error.cancelled,
        });
    }

    pub(super) fn diagnostics_truncated(&self) -> bool {
        self.seen_diagnostics
            .lock()
            .expect("diagnostic deduplication lock")
            .truncated
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn after_cleanup_reports_distinct_failures_in_either_arrival_order() {
        for order in [[1, 2], [2, 1]] {
            let records = Arc::new(Mutex::new(Vec::new()));
            let sink = records.clone();
            let trace = Trace::new(
                None,
                Arc::new(move |event| sink.lock().unwrap().push(event)),
            );
            trace.execution_budget.0.clone().fail_independent();
            let mut error = ScriptError::new("runtime", "fixed failure");
            error.cancelled = true;
            trace.hook_diagnostic(HookSlot::FieldAfter, &Location::default(), error.clone());
            assert!(!trace.observations_stopped());
            error.cancelled = false;
            for line in order {
                error.body = (line, 1);
                trace.hook_diagnostic(HookSlot::FieldAfter, &Location::default(), error.clone());
            }
            assert!(trace.observations_stopped());
            assert_eq!(records.lock().unwrap().len(), 2);
        }
    }

    #[test]
    fn repeated_identical_frames_are_collapsed_and_the_stack_is_bounded() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let trace = Trace::new(
            None,
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        );
        let error = ScriptError::new("runtime", "fixed failure");
        for line in 1..=crate::limits::QUEUED_JOBS {
            let mut error = error.clone();
            error.body = (line, 1);
            for _ in 0..2 {
                trace.hook_diagnostic(HookSlot::FieldBefore, &Location::default(), error.clone());
            }
        }
        assert_eq!(records.lock().unwrap().len(), crate::limits::QUEUED_JOBS);
        assert!(!trace.diagnostics_truncated());
        let mut next = error;
        next.body = (crate::limits::QUEUED_JOBS + 1, 1);
        trace.hook_diagnostic(HookSlot::FieldBefore, &Location::default(), next);
        assert!(trace.diagnostics_truncated());
        assert_eq!(records.lock().unwrap().len(), crate::limits::QUEUED_JOBS);
    }

    #[test]
    fn ab_09_native_sites_have_null_sources_and_verified_script_positions_are_retained() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let mut trace = Trace::new(None, Arc::new(|_| {}));
        Arc::get_mut(&mut trace).unwrap().diagnostics =
            Arc::new(move |event| sink.lock().unwrap().push(event));

        trace.resolver_diagnostic(
            FieldId(0),
            "Image.url".into(),
            None,
            ScriptError::new("output", "native output failed"),
        );
        let mut script = ScriptError::new("runtime", "resolver failed");
        script.body = (7, 3);
        script.mark_source_position();
        trace.resolver_diagnostic(
            FieldId(1),
            "Query.image".into(),
            Some("resolvers/query/image.luau".into()),
            script,
        );

        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].module.is_none());
        assert_eq!(records[0].line, None);
        assert_eq!(records[0].column, None);
        assert_eq!(records[0].member, None);
        assert_eq!(records[1].line, Some(7));
        assert_eq!(records[1].column, Some(3));
        assert_eq!(
            records[1].module.as_deref(),
            Some("resolvers/query/image.luau")
        );
        assert_eq!(records[1].expected.as_deref(), Some("valid contract"));
    }

    #[test]
    fn one_shared_hard_failure_is_not_duplicated_during_field_cleanup() {
        use crate::limits::request::LocalLimit;
        for cause in [
            BudgetFailure::Hard(crate::limits::request::LimitNotice {
                kind: LimitKind::Work,
                observed: 1_000_001,
            }),
            BudgetFailure::LocalHard(LocalLimit::VmMemory),
            BudgetFailure::LocalHard(LocalLimit::HostCalls),
            BudgetFailure::LocalHard(LocalLimit::PcallReentry),
        ] {
            let trace = Trace::new(None, Arc::new(|_| {}));
            trace.execution_budget.0.clone().fail_with(cause);
            let mut error = ScriptError::new("budget", "fixed failure");
            assert!(trace.claim_diagnostic(Owner::Resolver(FieldId(0)), &error));
            assert!(!trace.claim_diagnostic(Owner::Resolver(FieldId(1)), &error));
            error.cancelled = true;
            error.phase = "runtime";
            assert!(!trace.claim_diagnostic(Owner::Hook(HookSlot::FieldBefore), &error));
            assert_eq!(trace.seen_diagnostics.lock().unwrap().keys.len(), 1);
        }
    }
}
