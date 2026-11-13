//! Sequential endpoint ownership shared by every protocol bridge.

mod event;
pub(crate) mod input;
mod setup;
pub(crate) mod setup_deadlines;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use chigiri_http::{ResponseCompletionOutcome, ResponseHeaders, ResponseParts};
use chigiri_runtime::__private::SubscriptionSession;
use chigiri_runtime::{GraphQlResponse, OperationIdentity, SubscriptionOverflow};

use super::preparation::initial::InitialCompletion;
use super::request::SubscriberRequest;
use crate::subscription::execution_state::SubscriptionExecutionState;

pub(crate) use setup::start;
#[cfg(feature = "subscription-worker")]
pub(crate) use setup::{MonitoredSourceStart, initialize_monitored, start_source_monitored};

/// One initial protocol response and its source ownership, before body handoff.
#[doc(hidden)]
pub enum SubscriptionInitialization<C> {
    /// Source setup and initial hook succeeded; the bridge may commit headers.
    Ready {
        /// The sole sequential source owner.
        subscription: Box<EndpointSubscription<C>>,
        /// Additions from the one initial HTTP response hook.
        headers: ResponseHeaders,
        /// Must finish once after bounded initial response preparation.
        completion: SubscriptionCompletion,
    },
    /// A safe request/setup error to encode using the selected stream profile.
    Response {
        /// The ordinary GraphQL response before protocol-specific projection.
        response: GraphQlResponse,
        /// Initial hook additions, if setup reached application context.
        headers: ResponseHeaders,
        /// Initial response preparation completion, never a stream-lifetime hook.
        completion: SubscriptionCompletion,
    },
    /// A finite setup failure whose armed completion remains worker-owned.
    CompletionResponse {
        /// The bounded provisional response before the completion callback.
        response: ResponseParts,
        /// The unique initial response completion token.
        completion: SubscriptionCompletion,
        /// The outcome used if the overall deadline does not win first.
        outcome: ResponseCompletionOutcome,
    },
    /// A pre-stream HTTP rejection; no live source remains.
    Rejected(ResponseParts),
}

/// One-shot completion of bounded initial response preparation.
#[doc(hidden)]
pub struct SubscriptionCompletion {
    completion: Option<InitialCompletion>,
}

impl SubscriptionCompletion {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn into_token(self) -> Option<std::sync::Arc<dyn chigiri_http::ResponseCompletion>> {
        self.completion.and_then(InitialCompletion::into_token)
    }

    pub(super) const fn empty() -> Self {
        Self { completion: None }
    }

    pub(super) const fn armed(completion: InitialCompletion) -> Self {
        Self {
            completion: Some(completion),
        }
    }

    /// Completes initial preparation without waiting for the streaming lifetime.
    pub fn finish(
        self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        match self.completion {
            Some(completion) => completion.finish(response, outcome),
            None => response,
        }
    }
}

/// Owned subscriber metadata and at most one active source/event generation.
#[doc(hidden)]
pub struct EndpointSubscription<C> {
    state: Arc<SubscriptionExecutionState<C>>,
    request: SubscriberRequest,
    session: Option<SubscriptionSession<C>>,
    sequence: u64,
}

impl<C: Send + Sync + 'static> EndpointSubscription<C> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn retained_telemetry(&self) -> Option<&chigiri_runtime::RequestTelemetry> {
        self.request.request().request_telemetry()
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn host_observation_receipt(
        &self,
    ) -> Option<&chigiri_runtime::__private::SubscriptionObservationReceipt> {
        self.session
            .as_ref()
            .and_then(SubscriptionSession::host_observation_receipt)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn from_websocket_source(
        state: Arc<SubscriptionExecutionState<C>>,
        request: SubscriberRequest,
        session: SubscriptionSession<C>,
    ) -> Self {
        Self {
            state,
            request,
            session: Some(session),
            sequence: 0,
        }
    }

    /// Moves idle source destruction into resource-free terminal cleanup work.
    ///
    /// The bridge must first revoke emission and retain the matching physical
    /// operation charge. An outstanding `next` borrow must finish before this
    /// owned handoff; a running worker needs its separate completion handle.
    pub fn into_cleanup(self) -> crate::subscription::bridge_resources::DetachedCleanup {
        crate::subscription::bridge_resources::DetachedCleanup::new(async move {
            drop(self);
        })
    }

    /// Returns the immutable operation identity.
    pub fn identity(&self) -> &OperationIdentity {
        self.request.identity()
    }

    /// Returns the charged retained request size without disclosing its values.
    pub fn retained_metadata_bytes(&self) -> usize {
        self.request.charged_bytes()
    }

    /// Returns the server-selected overflow policy, if the source is still owned.
    pub fn overflow(&self) -> Option<SubscriptionOverflow> {
        self.session.as_ref().map(SubscriptionSession::overflow)
    }

    /// Revokes ownership and drops the source synchronously.
    pub fn cancel(&mut self) {
        self.session = None;
    }
}

/// One sequential execution result; source errors are not natural completion.
#[doc(hidden)]
pub enum SubscriptionOutput {
    /// A normal event response, including ordinary partial field errors.
    Event(GraphQlResponse),
    /// Natural source completion.
    Complete,
    /// A safe failure that terminates this subscription generation.
    Terminal(chigiri_runtime::__private::SubscriptionTerminalResponse),
    /// A cancelled generation cannot emit another result.
    Cancelled,
}
