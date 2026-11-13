//! Carry request state and phase views across the complete hook and candidate lifecycle.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use chigiri::{GraphQlError, GraphQlResponse, PathSegment, Value};

use super::{HookEvent, error, http::Trace};
use crate::hooks::{HookSlot, HookTiming, RequestState};
use crate::luau::ScriptError;
use crate::runtime::{Execution, HookCall, HookOutcome, HookViews};
use crate::service::Client;
use crate::store::Snapshot;

mod preparation;

tokio::task_local! { pub(super) static REQUEST: Arc<Session>; }

pub(super) fn current() -> Arc<Session> {
    REQUEST.with(Arc::clone)
}

pub(in crate::service) struct Session {
    pub(super) trace: Arc<Trace>,
    pub(super) state: Mutex<RequestState>,
    pub(super) operation: OnceLock<Result<Arc<Value>, ScriptError>>,
    pub(super) client: OnceLock<Client>,
    pub(in crate::service::hooks) source_cancel: OnceLock<crate::cancellation::Cancellation>,
    pub(super) started: std::sync::atomic::AtomicBool,
    pub(super) failure: Mutex<Option<super::completion::Failure>>,
    pub(super) http: bool,
    pub(super) request: bool,
    _scope: Option<super::HttpScope>,
}

#[derive(Clone, Default)]
pub(super) struct Location {
    pub(super) coordinate: Option<Arc<str>>,
    pub(super) path: Arc<[PathSegment]>,
}

#[derive(Clone)]
pub(super) struct FieldView {
    pub(super) value: Arc<Value>,
    pub(super) location: Location,
}

#[derive(Default)]
pub(super) struct PhaseInput {
    pub(super) field: Option<FieldView>,
    pub(super) result: Option<Arc<Value>>,
    pub(super) snapshot: Option<Snapshot>,
}

impl PhaseInput {
    fn location(&self) -> Location {
        self.field
            .as_ref()
            .map(|field| field.location.clone())
            .unwrap_or_default()
    }
}

impl Session {
    pub(super) fn source_stopped(&self) -> bool {
        self.source_cancel
            .get()
            .is_some_and(crate::cancellation::Cancellation::cancelled)
    }

    fn cancellation(&self) -> crate::cancellation::Cancellation {
        self.source_cancel.get().map_or_else(
            || self.trace.cancel.clone(),
            |source| self.trace.cancel.linked(source),
        )
    }
    pub(super) fn execution(&self, client: &Client) -> Result<Execution, ScriptError> {
        let request = if self.request {
            self.trace.raw.get().cloned().transpose()?
        } else {
            None
        };
        Ok(Execution {
            state: self.state.lock().expect("request state").snapshot(),
            request,
            deadline: self.trace.deadline,
            cancel: self.cancellation(),
            shutdown: client.shutdown.clone(),
            budget: self.trace.execution_budget.clone(),
        })
    }

    pub(super) async fn before(
        &self,
        client: &Client,
        slot: HookSlot,
        input: PhaseInput,
    ) -> Result<(), GraphQlError> {
        if !client.enabled(slot) {
            return Ok(());
        }
        let location = input.location();
        let outcome = self.invoke(client, slot, input).await;
        #[cfg(test)]
        super::tests::scheduling::hook_result().await;
        match outcome {
            Ok(HookOutcome::Continue(next)) => {
                if let Some(next) = next {
                    *self.state.lock().expect("request state") = next;
                }
                Ok(())
            }
            Ok(HookOutcome::Rejected(rejected, diagnostic)) => {
                self.trace.reject(rejected.status);
                if let Some(diagnostic) = diagnostic {
                    self.diagnostic(slot, &location, diagnostic);
                }
                let error = GraphQlError::new(rejected.message)
                    .code("MOCK_REJECTED")
                    .extension("httpStatus", Value::Int(rejected.status.into()));
                self.fail(super::completion::Failure::Rejected(
                    rejected.status,
                    error.clone(),
                ));
                Err(error)
            }
            Err(error) => {
                self.diagnostic(slot, &location, error);
                Err(GraphQlError::new("Mock hook failed").code("MOCK_HOOK_ERROR"))
            }
        }
    }

    pub(super) async fn after_response(
        &self,
        client: &Client,
        slot: HookSlot,
        response: &GraphQlResponse,
        progress: &mut super::progress::After,
    ) {
        self.observe_execution_failure();
        if !response.errors.is_empty() && self.failure.lock().expect("Request failed").is_none() {
            self.fail(super::completion::Failure::Internal);
        }
        if !client.enabled(slot) || !self.active(client) || self.trace.observations_stopped() {
            return;
        }
        let view = super::copy_budget::at(
            self.trace.observations(),
            crate::limits::warning::LimitOrigin {
                phase: slot.name(),
                ..Default::default()
            },
            |budget| super::observation::response(response, budget),
        );
        match view {
            Ok(view) => {
                self.after(
                    client,
                    slot,
                    PhaseInput {
                        result: Some(Arc::new(view)),
                        ..Default::default()
                    },
                    progress,
                )
                .await
            }
            Err(_) => self.diagnostic(
                slot,
                &Location::default(),
                error("response observation conversion failed"),
            ),
        }
    }

    pub(super) async fn after(
        &self,
        client: &Client,
        slot: HookSlot,
        input: PhaseInput,
        progress: &mut super::progress::After,
    ) {
        if !client.enabled(slot) || !self.active(client) || self.trace.observations_stopped() {
            return;
        }
        let location = input.location();
        progress.reached();
        if let Err(error) = self.invoke(client, slot, input).await {
            self.diagnostic(slot, &location, error);
        }
    }

    async fn invoke(
        &self,
        client: &Client,
        slot: HookSlot,
        input: PhaseInput,
    ) -> Result<HookOutcome, ScriptError> {
        let client = self.client.get().unwrap_or(client);
        let mut views = HookViews::new();
        for (name, _) in crate::api::hooks::phase_fields(slot) {
            let value = match name {
                "request" => self
                    .trace
                    .raw
                    .get()
                    .cloned()
                    .unwrap_or_else(|| Err(error("HTTP request snapshot unavailable")))?,
                "operation" => self
                    .operation
                    .get()
                    .cloned()
                    .unwrap_or_else(|| Err(error("operation snapshot unavailable")))?,
                "field" => input
                    .field
                    .as_ref()
                    .map(|field| field.value.clone())
                    .ok_or_else(|| error("field snapshot unavailable"))?,
                "result" => input
                    .result
                    .clone()
                    .ok_or_else(|| error("result snapshot unavailable"))?,
                _ => return Err(error("unknown hook view")),
            };
            views.insert(name, value);
        }
        let location = input.location();
        let snapshot = match input.snapshot {
            Some(snapshot) => snapshot,
            None => client.snapshot(self.trace.deadline).await?,
        };
        let state = self.state.lock().expect("request state").clone();
        let events = self.trace.events.clone();
        let request_id = self.trace.id;
        let trace = self.trace.clone();
        let diagnostic_location = location.clone();
        let client = client.with_diagnostics(move |error| {
            trace.hook_diagnostic(slot, &diagnostic_location, error);
        });
        let outcome =
            client
                .hook(
                    slot,
                    HookCall {
                        snapshot,
                        state,
                        request: if self.request {
                            Some(self.trace.raw.get().cloned().unwrap_or_else(|| {
                                Err(error("HTTP request snapshot unavailable"))
                            })?)
                        } else {
                            None
                        },
                        views,
                        deadline: self.trace.deadline,
                        cancel: self.cancellation(),
                        shutdown: client.shutdown.clone(),
                        budget: self.trace.execution_budget.at(
                            crate::limits::warning::LimitOrigin {
                                phase: slot.name(),
                                coordinate: location.coordinate.clone(),
                                path: location.path.clone(),
                            },
                        ),
                        log: Arc::new(move |record| {
                            events(HookEvent {
                                request_id,
                                slot,
                                module: None,
                                coordinate: location.coordinate.clone(),
                                path: location.path.clone(),
                                record: Ok(record),
                            })
                        }),
                    },
                )
                .await?;
        if slot.timing() == HookTiming::After && !matches!(outcome, HookOutcome::Continue(None)) {
            return Err(error("observational hook returned a decision"));
        }
        Ok(outcome)
    }

    pub(super) fn active(&self, client: &Client) -> bool {
        !self.trace.cancel.cancelled()
            && !self.source_stopped()
            && !client.shutdown.cancelled()
            && Instant::now() < self.trace.deadline
    }

    pub(super) fn execution_error(&self, error: &ScriptError) {
        if error.phase == "business" {
            return;
        }
        if !(error.cancelled
            && matches!(
                *self.failure.lock().expect("requestFailure"),
                Some(super::completion::Failure::Rejected(..))
            ))
        {
            self.fail(super::completion::Failure::Internal);
        }
    }

    pub(super) fn diagnostic(&self, slot: HookSlot, location: &Location, error: ScriptError) {
        self.execution_error(&error);
        self.trace.hook_diagnostic(slot, location, error);
    }
}
