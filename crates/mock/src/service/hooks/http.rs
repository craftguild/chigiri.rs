//! Carry request deadlines, encoded status, and rejection results through transport extensions.

mod observe;
mod registry;

pub(in crate::service) use observe::Observe;
pub(crate) use registry::HttpScopes;

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use chigiri::{HttpRequestContext, Value};

use super::{EventSink, HookEvent};
use crate::cancellation::{CancelOnDrop, Cancellation};
use crate::luau::ScriptError;
use crate::value::ValueBudget;

tokio::task_local! { static HTTP: Arc<Trace>; }
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Mock-specific scope that links the same HTTP request to hook/access records.
///
/// The built-in subscription log sink is not a public configuration surface:
///
/// ```compile_fail
/// use chigiri_mock::{logging::Writer, service::HttpScope};
///
/// fn replace_sink(scope: HttpScope, writer: &Writer) {
///     let _ = scope.with_subscription_logging(writer.logger.clone());
/// }
/// ```
pub struct HttpScope(pub(super) Arc<Trace>);

pub(super) struct Trace {
    pub id: u64,
    pub bound: Option<SocketAddr>,
    pub deadline: Instant,
    pub cancel: Cancellation,
    pub raw: OnceLock<Result<Arc<Value>, ScriptError>>,
    pub budget: Mutex<ValueBudget>,
    observation_failure_reported: AtomicBool,
    pub execution_budget: crate::runtime::ExecutionBudget,
    pub events: EventSink,
    pub diagnostics: super::DiagnosticSink,
    pub(super) seen_diagnostics: Mutex<super::diagnostics::Seen>,
    pub terminal: Arc<dyn Fn(super::TerminalDiagnostic) + Send + Sync>,
    pub(super) subscription_logs: Option<crate::logging::Logger>,
    pub(super) subscription_observation: OnceLock<super::subscription_observation::EntryFence>,
    pub terminal_reported: AtomicBool,
    pub(super) unreached_after: AtomicU8,
    status: AtomicU16,
    encoded: AtomicBool,
    failed: AtomicBool,
    admission: OnceLock<tokio::sync::OwnedSemaphorePermit>,
}

impl Trace {
    pub fn current() -> Option<Arc<Self>> {
        HTTP.try_with(Arc::clone).ok()
    }
    pub fn new(bound: Option<SocketAddr>, events: EventSink) -> Arc<Self> {
        let cancel = Cancellation::default();
        let started = Instant::now();
        let deadline = started + crate::limits::OPERATION_TIME;
        let mut work = crate::value::work::Work::default();
        work.start_request(started);
        work.guard(&cancel, &Cancellation::default(), deadline);
        Arc::new(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            bound,
            deadline,
            cancel,
            raw: OnceLock::new(),
            budget: Mutex::new(ValueBudget::with_work(work.clone())),
            observation_failure_reported: AtomicBool::new(false),
            execution_budget: crate::runtime::ExecutionBudget(
                work,
                Default::default(),
                Default::default(),
                Default::default(),
                None,
            ),
            events,
            diagnostics: Arc::new(|_| {}),
            seen_diagnostics: Mutex::default(),
            terminal: Arc::new(|_| {}),
            subscription_logs: None,
            subscription_observation: OnceLock::new(),
            terminal_reported: AtomicBool::new(false),
            unreached_after: AtomicU8::new(0),
            status: AtomicU16::new(0),
            encoded: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            admission: OnceLock::new(),
        })
    }
    pub fn observations(&self) -> &Mutex<ValueBudget> {
        &self.budget
    }

    pub fn view_budget(&self, _needed_before: bool) -> &Mutex<ValueBudget> {
        &self.budget
    }

    pub(super) fn transport_failed(&self) {
        self.failed.store(true, Ordering::Relaxed);
    }

    pub fn observations_stopped(&self) -> bool {
        self.observation_failure_reported.load(Ordering::Acquire)
    }

    /// Stop observations. Diagnostic delivery is independent of work already
    /// admitted.
    pub fn stop_observations(&self) {
        self.observation_failure_reported
            .store(true, Ordering::Release);
    }

    pub fn reject(&self, status: u16) {
        self.status.fetch_max(status, Ordering::Relaxed);
    }
}

impl HttpScope {
    pub(crate) fn hold_admission(
        &self,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<(), ScriptError> {
        self.0
            .admission
            .set(permit)
            .map_err(|_| ScriptError::new("environment", "HTTP scope already owns admission"))
    }

    pub(in crate::service) fn fresh_from_current() -> Self {
        let inherited = Trace::current();
        let mut trace = Trace::new(
            inherited.as_ref().and_then(|trace| trace.bound),
            inherited
                .as_ref()
                .map(|trace| trace.events.clone())
                .unwrap_or_else(|| Arc::new(|_| {})),
        );
        if let Some(inherited) = inherited {
            let fresh = Arc::get_mut(&mut trace).expect("fresh phase trace is unshared");
            fresh.diagnostics = inherited.diagnostics.clone();
            fresh.terminal = inherited.terminal.clone();
            fresh.subscription_logs = inherited.subscription_logs.clone();
        }
        Self(trace)
    }

    /// Create a scope from the numeric address of the real listener. The value is
    /// never derived from `Host` or `Forwarded` headers.
    pub fn new(bound: SocketAddr) -> Self {
        Self(Trace::new(Some(bound), Arc::new(|_| {})))
    }
    /// Attach an event sink that performs no I/O in this scope.
    pub fn with_events(mut self, events: impl Fn(HookEvent) + Send + Sync + 'static) -> Self {
        Arc::get_mut(&mut self.0)
            .expect("scope is not yet shared")
            .events = Arc::new(events);
        self
    }
    /// Attach a sink for safe resolver diagnostics.
    pub fn with_diagnostics(
        mut self,
        sink: impl Fn(super::ResolverDiagnostic) + Send + Sync + 'static,
    ) -> Self {
        Arc::get_mut(&mut self.0)
            .expect("HTTP scope not yet published")
            .diagnostics = Arc::new(sink);
        self
    }
    /// Return the soft-limit warning recorded by the shared request budget.
    pub fn with_limit_warnings(
        self,
        sink: impl Fn(u64, crate::limits::warning::LimitWarning) + Send + Sync + 'static,
    ) -> Self {
        let id = self.0.id;
        self.0
            .execution_budget
            .0
            .with_warnings(move |notice| sink(id, notice));
        self
    }
    /// Install the sink for the request's terminal diagnostic.
    pub fn with_terminal(
        mut self,
        sink: impl Fn(super::TerminalDiagnostic) + Send + Sync + 'static,
    ) -> Self {
        Arc::get_mut(&mut self.0)
            .expect("HTTP scope not yet published")
            .terminal = Arc::new(sink);
        self
    }
    /// Attach the built-in bounded subscription log sink, not a user callback.
    pub(crate) fn with_subscription_logging(mut self, logger: crate::logging::Logger) -> Self {
        Arc::get_mut(&mut self.0)
            .expect("HTTP scope not yet published")
            .subscription_logs = Some(logger);
        self
    }
    /// Scope the complete HTTP future so unfinished jobs are cancelled when it ends.
    pub async fn run<T>(&self, future: impl Future<Output = T>) -> T {
        let _cancel = CancelOnDrop(self.0.cancel.clone());
        let policy = crate::limits::request::policy(crate::limits::request::LimitKind::ElapsedMs);
        let soft =
            self.0.deadline - std::time::Duration::from_millis((policy.hard - policy.soft) as u64);
        let future = HTTP.scope(self.0.clone(), future);
        tokio::pin!(future);
        // Observe one request-wide wait without resetting the deadline for later phases.
        tokio::select! {
            result = &mut future => {
                self.0.execution_budget.0.observe_elapsed_soft();
                result
            },
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(soft)) => {
                let _ = self.0.execution_budget.0.clone().charge(0);
                future.await
            }
        }
    }
    /// Return a validated status override only after successful response encoding.
    pub fn response_status(&self) -> Option<u16> {
        let status = self.0.status.load(Ordering::Relaxed);
        (status != 0
            && self.0.encoded.load(Ordering::Relaxed)
            && !self.0.failed.load(Ordering::Relaxed))
        .then_some(status)
    }
    /// Return the Mock correlation ID, distinct from Chigiri's `RequestIdentity`.
    pub fn request_id(&self) -> u64 {
        self.0.id
    }

    #[cfg(test)]
    pub(super) fn encoded_for_test(&self) -> bool {
        self.0.encoded.load(Ordering::Relaxed)
    }

    /// Record at most one terminal event for HTTP cancelled before context creation.
    pub fn report_not_started(&self, status: Option<u16>) {
        if !self.0.terminal_reported.swap(true, Ordering::AcqRel) {
            (self.0.terminal)(super::TerminalDiagnostic {
                request_id: self.0.id,
                status,
                transaction: "not_started",
                cause: "not_started",
                level: "info",
                phase: if status.is_some() { "response" } else { "drop" },
                limit: None,
                diagnostics_truncated: self.0.diagnostics_truncated(),
                unreached_after_hooks: self.0.unreached_after_hooks(),
            });
        }
    }
}

impl Drop for HttpScope {
    fn drop(&mut self) {
        self.report_not_started(None);
    }
}

pub(super) fn prepare(trace: &Trace, context: HttpRequestContext<'_>) {
    let result = super::copy_budget::at(
        &trace.budget,
        crate::limits::warning::LimitOrigin {
            phase: "request.view",
            ..Default::default()
        },
        |budget| super::request_view::request(context, trace.bound, budget),
    );
    let _ = trace.raw.set(
        result
            .map(Arc::new)
            .map_err(|_| super::error("request snapshot conversion failed")),
    );
}
