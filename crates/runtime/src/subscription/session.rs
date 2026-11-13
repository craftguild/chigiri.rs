//! Sequential source and event ownership without spawned tasks or prefetch.

mod diagnostics;
mod drive;

use std::sync::Arc;

use chigiri_core::executor::{InternalErrorHandler, PreparedSubscriptionRoot};
use chigiri_core::{FieldError, GraphQlResponse, OperationIdentity, RequestTelemetry};

use crate::ExecutableSchema;
use crate::instrumentation::InstrumentationScope;

use super::erased::EventStream;
use super::{
    ErasedSubscriptionArguments, ErasedSubscriptionEvent, SubscriptionOverflow,
    SubscriptionTerminalResponse,
};

/// One established source and its exact prepared operation.
#[doc(hidden)]
pub struct SubscriptionSession<C> {
    pub(super) schema: ExecutableSchema<C>,
    pub(super) root: PreparedSubscriptionRoot,
    pub(super) source: Option<EventStream>,
    pub(super) arguments: ErasedSubscriptionArguments,
    pub(super) current_event: Option<ErasedSubscriptionEvent>,
    pub(super) overflow: SubscriptionOverflow,
    pub(super) identity: Arc<OperationIdentity>,
    pub(super) state: SessionState,
    pub(super) observation: Option<super::SubscriptionObservationReceipt>,
    pub(super) retired_source: Option<EventStream>,
    pub(super) defer_source_cleanup: bool,
    pub(super) source_instrumentation: Option<InstrumentationScope>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SessionState {
    Polling,
    EventReady,
    Cancelled,
    Complete,
    Failed,
}

/// Demand outcome before preparing a fresh event context.
#[doc(hidden)]
pub enum SubscriptionSourceState {
    /// Exactly one owned item is waiting for event-context preparation.
    EventReady,
    /// The source ended successfully.
    Complete,
    /// The source failed; an observed owner retains physical cleanup separately.
    Terminal(SubscriptionTerminalResponse),
    /// The source or event future was cancelled and cannot emit again.
    Cancelled,
}

/// Result of resolving the one already demanded source item.
#[doc(hidden)]
pub enum SubscriptionEventResult {
    /// Normal GraphQL event output, including ordinary partial field errors.
    Event(GraphQlResponse),
    /// A framework ownership failure terminated the source.
    Terminal(SubscriptionTerminalResponse),
    /// A cancelled generation has no event result.
    Cancelled,
}

impl<C: Send + Sync + 'static> SubscriptionSession<C> {
    /// Binds an admitted payload before application event preparation starts.
    /// None preserves the existing source ownership on the opt-out path.
    #[doc(hidden)]
    pub fn bind_event_instrumentation(&mut self, scope: Option<InstrumentationScope>) {
        if let Some(event) = self.current_event.take() {
            self.current_event = Some(event.with_scope(scope));
        }
    }

    /// Keeps terminal source destruction in the managed owner's disposal boundary.
    /// This does not create or replace a host observation receipt.
    #[doc(hidden)]
    pub fn defer_source_cleanup(&mut self) {
        self.defer_source_cleanup = true;
        if let Some(source) = self.source.as_mut() {
            source.as_mut().defer_destruction();
        }
    }

    /// Returns identity-only evidence for the matching outside observation owner.
    #[doc(hidden)]
    pub fn host_observation_receipt(&self) -> Option<&super::SubscriptionObservationReceipt> {
        self.observation.as_ref()
    }

    /// Returns the server-selected policy fixed during source setup.
    pub const fn overflow(&self) -> SubscriptionOverflow {
        self.overflow
    }

    /// Returns the immutable selected operation identity.
    pub fn identity(&self) -> &OperationIdentity {
        &self.identity
    }

    /// Cancels source and pending event ownership synchronously.
    pub fn cancel(&mut self) {
        self.state = SessionState::Cancelled;
        self.current_event = None;
        self.source = None;
        self.retired_source = None;
    }

    fn retain_for_outside_cleanup(&mut self, source: EventStream) {
        if self.observation.is_some() || self.defer_source_cleanup {
            self.retired_source = Some(source);
        }
    }

    fn terminal_response(
        &self,
        mut error: FieldError,
        telemetry: Option<&RequestTelemetry>,
    ) -> GraphQlResponse {
        let path = vec![self.root.response_key().into()];
        let coordinate = format!("{}.{}", self.root.parent_type(), self.root.field_name());
        if error.is_internal() {
            self.schema.inner.diagnostics.handle_internal_error(
                &mut error,
                &self.identity,
                telemetry,
                &path,
                Some(&coordinate),
                "subscription-source",
            );
        }
        GraphQlResponse::request_error(error.to_graphql_error(self.root.locations().to_vec(), path))
    }

    fn source_terminal_response(
        &self,
        error: FieldError,
        telemetry: Option<&RequestTelemetry>,
    ) -> GraphQlResponse {
        // This temporary metadata only serves the terminal callback. It does
        // not retain another request/ID copy throughout source waiting.
        let bound = self.source_instrumentation.as_ref().map(|scope| {
            telemetry
                .cloned()
                .unwrap_or_else(|| {
                    RequestTelemetry::new(None, None, chigiri_core::SamplingDecision::Drop)
                })
                .with_instrumentation_scope(scope.clone())
        });
        self.terminal_response(error, bound.as_ref().or(telemetry))
    }
}
