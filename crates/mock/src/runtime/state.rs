//! Hold one call's host permissions, cancellation state, request views, and candidate store session.

use std::sync::Arc;
use std::time::Instant;

use chigiri::core::{Object, Value};
use luaur::{Error, Lua};

use crate::cancellation::Cancellation;
use crate::ir::{MockIr, ModelId};
use crate::protected_call::State;
use crate::store::{Reader, Record, Transaction};
use crate::value::ValueError;

pub(super) enum Session {
    Read(Reader),
    Write(Transaction),
}

impl Session {
    pub fn values(&mut self) -> &mut crate::value::ValueBudget {
        match self {
            Self::Read(reader) => reader.value_budget(),
            Self::Write(transaction) => transaction.value_budget(),
        }
    }
}

pub(super) struct Host {
    pub ir: Arc<MockIr>,
    pub session: Session,
    pub failed: bool,
    pub cancelled_failure: bool,
    pub cancel_unwinding: bool,
    pub last_line: usize,
    pub stack: Option<crate::luau::ScriptStack>,
    pub stack_captures: usize,
    pub stack_source: Arc<str>,
    pub calls: usize,
    pub cancel: Cancellation,
    pub shutdown: Cancellation,
    pub deadline: Instant,
    pub request_state: Arc<Object>,
    pub request: Option<Arc<Value>>,
    pub clock: Arc<crate::clock::RequestClock>,
    pub business: super::business::BusinessLedger,
    pub business_path: Arc<[chigiri::PathSegment]>,
    pub business_unwinding: bool,
    pub publications: Option<crate::publication::PublicationBuffer>,
    pub hook: Option<super::hook::HookHost>,
}

impl Host {
    pub fn budget(&mut self) -> luaur::Result<()> {
        use crate::limits::request::BudgetFailure;
        let work_failed = self.session.values().work.charge(0).is_err();
        let failure = if self.cancel.cancelled() || self.shutdown.cancelled() {
            Some(BudgetFailure::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(BudgetFailure::Deadline)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.session.values().work.fail_with(failure);
        }
        if self.failed || work_failed || failure.is_some() {
            let cancelled = (self.cancelled_failure
                || failure == Some(BudgetFailure::Cancelled) && !self.failed)
                && matches!(
                    self.session.values().work.failure(),
                    Some(BudgetFailure::Cancelled)
                );
            self.fail();
            self.cancelled_failure = cancelled;
            self.cancel_unwinding = cancelled;
            Err(Error::runtime("mock execution cancelled or expired"))
        } else {
            Ok(())
        }
    }

    /// Consume the host session without borrowing it again.
    pub fn budget_error(
        &mut self,
        mut error: crate::luau::ScriptError,
    ) -> Result<(), crate::luau::ScriptError> {
        self.budget().map_err(|_| {
            error.cancelled = self.cancel_unwinding;
            error
        })
    }

    pub fn call(&mut self, lua: &Lua) -> luaur::Result<()> {
        self.observe_line(lua, 1);
        if let Err(error) = self.budget() {
            self.capture_stack(lua, 1, "host");
            return Err(error);
        }
        self.calls += 1;
        if self.calls > crate::api::HOST_CALL_LIMIT {
            self.session
                .values()
                .work
                .fail_local(crate::limits::request::LocalLimit::HostCalls);
            self.fail();
            self.capture_stack(lua, 1, "host");
            return Err(Error::runtime("host call budget exceeded"));
        }
        Ok(())
    }

    pub fn writable(&self) -> bool {
        matches!(self.session, Session::Write(_))
    }

    pub fn write(&mut self) -> Result<&mut Transaction, ValueError> {
        match &mut self.session {
            Session::Write(transaction) => Ok(transaction),
            Session::Read(_) => Err(ValueError("write capability required")),
        }
    }

    pub fn find(
        &mut self,
        model: ModelId,
        options: &Object,
    ) -> Result<Vec<Arc<Record>>, ValueError> {
        match &mut self.session {
            Session::Write(transaction) => transaction.find(model, options),
            Session::Read(reader) => reader.find(model, options),
        }
    }

    pub fn count(&mut self, model: ModelId, options: &Object) -> Result<usize, ValueError> {
        match &mut self.session {
            Session::Write(transaction) => transaction.count(model, options),
            Session::Read(reader) => reader.count(model, options),
        }
    }
}

impl State for Host {
    fn explicit_error(&mut self) {
        self.session.values().work.fail_independent();
        self.fail();
    }

    fn reentry_limit(&mut self) {
        self.session
            .values()
            .work
            .fail_local(crate::limits::request::LocalLimit::PcallReentry);
        self.fail();
    }

    fn invalid_call(&mut self, lua: &Lua) {
        self.session.values().work.fail_independent();
        self.fail();
        self.capture_stack(lua, 1, "host");
    }

    fn caught_error(&mut self, error: &Error) {
        // Do not treat expected reject or cleanup unwinding as an independent request failure.
        let control = matches!(error, Error::RuntimeError(_))
            && (self.cancel_unwinding
                || self.business_unwinding
                || self.hook.as_ref().is_some_and(|hook| hook.reject_unwinding));
        if !control {
            if matches!(error, Error::MemoryError(_)) {
                self.session
                    .values()
                    .work
                    .fail_local(crate::limits::request::LocalLimit::VmMemory);
            }
            self.session.values().work.fail_independent();
            self.fail();
        }
        self.caught();
    }

    fn caught(&mut self) {
        // Do not carry a captured stack into a later independent failure.
        self.stack = None;
        self.cancel_unwinding = false;
        self.business_unwinding = false;
        if let Some(hook) = &mut self.hook {
            hook.reject_unwinding = false;
        }
    }

    fn observe_line(&mut self, lua: &Lua, level: usize) {
        if let Some(line) = lua
            .inspect_stack(level)
            .and_then(|frame| frame.current_line())
            .and_then(|line| usize::try_from(line).ok())
            .filter(|line| *line > 0)
        {
            self.last_line = line;
        }
    }

    fn observe_failure(&mut self, lua: &Lua, level: usize) {
        self.capture_stack(lua, level, "reentry");
    }

    fn fail(&mut self) {
        // Preserve cancellation attribution when the same error propagates before capture.
        if !self.cancel_unwinding {
            self.cancelled_failure = false;
        }
        self.failed = true;
        // Return immediately rather than awaiting another VM interrupt; the request owner retains the host failure.
        self.session.values().work.fail();
        match &mut self.session {
            Session::Write(transaction) => transaction.fail(),
            Session::Read(reader) => reader.fail(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::request::BudgetFailure;

    #[test]
    fn host_failure_closes_the_vm_and_preserves_prior_cancellation_or_deadline() {
        let _case = crate::vm_gate::test_case();
        let contract = crate::runtime::hook_tests::contract();
        for previous in [
            None,
            Some(BudgetFailure::Cancelled),
            Some(BudgetFailure::Deadline),
        ] {
            let mut call =
                crate::runtime::hook_tests::call(&contract, crate::hooks::HookSlot::FieldBefore);
            let mut reader = call.snapshot.reader();
            reader.share_work(call.budget.0.clone());
            let mut host = Host {
                ir: call.snapshot.ir().clone(),
                session: Session::Read(reader),
                failed: false,
                cancelled_failure: false,
                cancel_unwinding: false,
                last_line: 0,
                stack: None,
                stack_captures: 0,
                stack_source: "field.before".into(),
                calls: 0,
                cancel: call.cancel,
                shutdown: call.shutdown,
                deadline: call.deadline,
                request_state: call.state.snapshot(),
                request: None,
                clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system())
                    .capture(),
                business: Default::default(),
                business_path: Default::default(),
                business_unwinding: false,
                publications: None,
                hook: None,
            };
            if let Some(cause) = previous {
                call.budget.0.fail_with(cause);
            }
            host.fail();
            assert_eq!(
                call.budget.failure(),
                Some(previous.unwrap_or(BudgetFailure::Execution))
            );
        }
    }
    #[test]
    fn outer_failures_unwind_while_reentry_preserves_origin_until_caught() {
        let _case = crate::vm_gate::test_case();
        let contract = crate::runtime::hook_tests::contract();
        let call = crate::runtime::hook_tests::call(&contract, crate::hooks::HookSlot::FieldBefore);
        let mut reader = call.snapshot.reader();
        reader.share_work(call.budget.0.clone());
        let host = Host {
            ir: call.snapshot.ir().clone(),
            session: Session::Read(reader),
            failed: false,
            cancelled_failure: false,
            cancel_unwinding: false,
            last_line: 0,
            stack: None,
            stack_captures: 0,
            stack_source: "field.before".into(),
            calls: 0,
            cancel: call.cancel,
            shutdown: call.shutdown,
            deadline: call.deadline,
            request_state: call.state.snapshot(),
            request: None,
            clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system()).capture(),
            business: Default::default(),
            business_path: Default::default(),
            business_unwinding: false,
            publications: None,
            hook: Some(crate::runtime::hook::HookHost {
                slot: crate::hooks::HookSlot::FieldBefore,
                rejection: None,
                reject_unwinding: false,
                log: Arc::new(|_| panic!("the log sink must not be reached after cancellation")),
            }),
        };
        let _lease = crate::vm_gate::test_guard();
        let lua = luaur::Lua::new();
        let host = std::rc::Rc::new(std::cell::RefCell::new(host));
        let codec = std::rc::Rc::new(std::cell::RefCell::new(
            crate::codec::Codec::new(&lua).unwrap(),
        ));
        let context = lua.create_table();
        crate::runtime::hook_functions::attach(&lua, &host, &codec, &context).unwrap();
        let log: luaur::Function = context.get("log").unwrap();
        host.borrow().cancel.cancel();
        // Invoke the real callback directly so host-call and external-failure
        // paths are not changed by an extra interrupt callback.
        assert!(log.call::<()>(("info", "cancelled")).is_err());
        let mut host = host.borrow_mut();
        assert!(host.cancel_unwinding && host.cancelled_failure);
        host.caught();
        assert!(!host.cancel_unwinding && host.cancelled_failure);
        host.fail();
        assert!(!host.cancel_unwinding && !host.cancelled_failure);
        assert_eq!(call.budget.failure(), Some(BudgetFailure::Cancelled));
    }
}
