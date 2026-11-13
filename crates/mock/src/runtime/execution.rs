//! Carry request-scoped state, deadlines, cancellation, and store snapshots into resolvers.

use std::sync::Arc;
use std::time::Instant;

use chigiri::core::{Object, Value};

use super::state::Session;
use crate::cancellation::Cancellation;
use crate::luau::{Script, ScriptError};
use crate::output::{OutputObject, OutputValue};
use crate::store::{CallScope, Snapshot, Store};

/// Shared request budget, clock, business-failure ledger, and diagnostic origin.
/// Cloning this value preserves the same request-scoped accounting state.
#[derive(Clone, Default)]
pub struct ExecutionBudget(
    pub(crate) crate::value::work::Work,
    pub(crate) std::sync::Arc<std::sync::OnceLock<std::sync::Arc<crate::clock::RequestClock>>>,
    pub(crate) super::business::BusinessLedger,
    pub(crate) std::sync::Arc<crate::limits::warning::LimitOrigin>,
    pub(crate) Option<crate::publication::PublicationBuffer>,
);

impl ExecutionBudget {
    pub(crate) fn at(&self, origin: crate::limits::warning::LimitOrigin) -> Self {
        Self(
            self.0.at(origin.clone()),
            self.1.clone(),
            self.2.clone(),
            std::sync::Arc::new(origin),
            self.4.clone(),
        )
    }

    pub(crate) fn with_publications(
        &self,
        publications: Option<crate::publication::PublicationBuffer>,
    ) -> Self {
        let mut budget = self.clone();
        budget.4 = publications;
        budget
    }

    pub(crate) fn set_clock(&self, server: &std::sync::Arc<crate::clock::ServerClock>) {
        self.1.get_or_init(|| server.capture());
    }

    pub(crate) fn clock(&self) -> std::sync::Arc<crate::clock::RequestClock> {
        self.1
            .get_or_init(|| {
                crate::clock::ServerClock::new(&crate::clock::ClockSettings::system()).capture()
            })
            .clone()
    }

    pub(crate) fn seal_clock(&self) {
        self.clock().seal();
    }

    pub(crate) fn business(&self) -> Option<super::business::BusinessFailure> {
        self.2.get()
    }

    /// Return the first shared phase failure without parsing error strings.
    pub fn failure(&self) -> Option<crate::limits::request::BudgetFailure> {
        self.0.failure()
    }

    /// Drain at most one pending soft-limit notice per resource for this request.
    pub fn take_limit_warnings(&self) -> Vec<crate::limits::request::LimitNotice> {
        self.0.take_warnings()
    }
}

/// Immutable inputs and request-scoped controls for one resolver execution.
/// This value owns neither mutable store state nor a VM handle.
#[derive(Clone)]
pub struct Execution {
    /// State returned by `operation.before`, read-only after preparation.
    pub state: Arc<Object>,
    /// One bounded transport snapshot, or no snapshot outside HTTP execution.
    pub request: Option<Arc<Value>>,
    /// Absolute deadline established for the request.
    pub deadline: Instant,
    /// Request or job cancellation signal.
    pub cancel: Cancellation,
    /// Process-wide worker shutdown signal.
    pub shutdown: Cancellation,
    /// Context shared by hooks, root resolvers, and nested-field resolvers.
    pub budget: ExecutionBudget,
}

impl Execution {
    fn isolated(script: &Script, cancel: &Cancellation, shutdown: &Cancellation) -> Self {
        Self {
            state: script.initial_state(),
            request: None,
            deadline: Instant::now() + super::VM_TIME,
            cancel: cancel.clone(),
            shutdown: shutdown.clone(),
            budget: ExecutionBudget::default(),
        }
    }
}

/// Execute a root resolver with a new isolated execution context.
pub fn execute_root(
    script: &Script,
    arguments: &Object,
    store: &mut Store,
    cancel: &Cancellation,
    shutdown: &Cancellation,
) -> Result<OutputValue, ScriptError> {
    execute_root_with(
        script,
        arguments,
        store,
        &store.snapshot(),
        &Execution::isolated(script, cancel, shutdown),
    )
}

/// Execute a nested resolver in isolation while retaining the parent's snapshot.
pub fn execute_nested(
    script: &Script,
    arguments: &Object,
    parent: &OutputObject,
    cancel: &Cancellation,
    shutdown: &Cancellation,
) -> Result<OutputValue, ScriptError> {
    execute_nested_with(
        script,
        arguments,
        parent,
        &Execution::isolated(script, cancel, shutdown),
    )
}

/// Preserve the read snapshot or mutation base selected by `field.before` instead of silently refreshing it.
pub fn execute_root_with(
    script: &Script,
    arguments: &Object,
    store: &mut Store,
    snapshot: &Snapshot,
    execution: &Execution,
) -> Result<OutputValue, ScriptError> {
    let bounded = Execution {
        deadline: execution.deadline.min(Instant::now() + super::VM_TIME),
        ..execution.clone()
    };
    let execution = &bounded;
    let ir = script.ir();
    let parent = ir.graph().fields()[ir.scripts()[script.id().0].field.0].parent;
    if !Arc::ptr_eq(ir, snapshot.ir())
        || !Arc::ptr_eq(ir, store.snapshot().ir())
        || (parent != ir.graph().query() && Some(parent) != ir.graph().mutation())
    {
        execution.budget.0.clone().fail();
        return Err(script.error("contract", "script does not belong to this root", 1, 1));
    }
    let session = if Some(parent) == ir.graph().mutation() {
        if !store.snapshot().same_version(snapshot) {
            execution.budget.0.clone().fail();
            return Err(script.error(
                "contract",
                "mutation snapshot changed before resolver",
                1,
                1,
            ));
        }
        let mut work = execution.budget.0.clone();
        work.guard(&execution.cancel, &execution.shutdown, execution.deadline);
        let transaction = store
            .begin_with_work(CallScope::MutationRoot, work)
            .map_err(|_| script.error("budget", "candidate copy budget or cancellation", 1, 1))?;
        Session::Write(transaction)
    } else {
        Session::Read(snapshot.reader())
    };
    super::execute(script, arguments, None, session, Some(store), execution)
}

/// Execute a nested resolver against the parent snapshot selected by `field.before`.
pub fn execute_nested_with(
    script: &Script,
    arguments: &Object,
    parent: &OutputObject,
    execution: &Execution,
) -> Result<OutputValue, ScriptError> {
    let ir = script.ir();
    let field = &ir.graph().fields()[ir.scripts()[script.id().0].field.0];
    if !Arc::ptr_eq(ir, parent.snapshot().ir()) || field.parent != parent.ty() {
        execution.budget.0.clone().fail();
        return Err(script.error("contract", "script does not belong to this parent", 1, 1));
    }
    super::execute(
        script,
        arguments,
        Some(parent),
        Session::Read(parent.snapshot().reader()),
        None,
        execution,
    )
}
