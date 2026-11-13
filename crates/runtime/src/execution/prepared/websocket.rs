//! One fully validated checkpoint for every GraphQL-over-WebSocket operation kind.

use std::sync::Arc;

use chigiri_core::language::ast::OperationType;
use chigiri_core::{GraphQlRequest, GraphQlResponse, OperationIdentity};

use super::{PreparationPolicy, PreparedOneShotOperation};
use crate::ExecutableSchema;
use crate::subscription::PreparedSubscription;

/// Fully validated resource-free WebSocket operation awaiting exact-kind admission.
#[doc(hidden)]
pub enum PreparedWebSocketOperation<C> {
    /// A query or mutation that executes once after active admission.
    OneShot(Box<PreparedOneShotOperation<C>>),
    /// A subscription that establishes one source after active admission.
    Subscription(Box<PreparedSubscription<C>>),
}

impl<C: Send + Sync + 'static> PreparedWebSocketOperation<C> {
    /// Budgets optional correlation before sharing the operation request.
    #[doc(hidden)]
    pub fn retain_subscription_instrumentation(&mut self, available: usize) -> Option<()> {
        match self {
            Self::OneShot(operation) => operation.retain_instrumentation(available),
            Self::Subscription(operation) => operation.retain_instrumentation(available),
        }
    }

    /// Returns the required charge independently of optional correlation.
    #[doc(hidden)]
    pub fn required_retained_metadata_bytes(&self) -> Option<usize> {
        match self {
            Self::OneShot(operation) => operation.required_retained_metadata_bytes(),
            Self::Subscription(operation) => operation.required_retained_metadata_bytes(),
        }
    }

    /// Returns the operation kind fixed by complete preparation.
    pub const fn kind(&self) -> WebSocketOperationKind {
        match self {
            Self::OneShot(operation) => match operation.kind() {
                super::OneShotOperationKind::Query => WebSocketOperationKind::Query,
                super::OneShotOperationKind::Mutation => WebSocketOperationKind::Mutation,
            },
            Self::Subscription(_) => WebSocketOperationKind::Subscription,
        }
    }

    /// Shares the decoded request without copying retained allocations.
    pub fn retained_request(&self) -> Arc<GraphQlRequest> {
        match self {
            Self::OneShot(operation) => operation.retained_request(),
            Self::Subscription(operation) => operation.retained_request(),
        }
    }

    /// Shares the identity allocation already retained by the prepared plan.
    pub fn retained_identity(&self) -> Arc<OperationIdentity> {
        match self {
            Self::OneShot(operation) => operation.retained_identity(),
            Self::Subscription(operation) => operation.retained_identity(),
        }
    }

    /// Charges the exact runtime/core portion of retained operation metadata.
    pub fn retained_metadata_bytes(&self) -> Option<usize> {
        match self {
            Self::OneShot(operation) => operation.retained_metadata_bytes(),
            Self::Subscription(operation) => operation.retained_metadata_bytes(),
        }
    }
}

/// The exact GraphQL operation kind fixed by complete WebSocket preparation.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketOperationKind {
    /// A query executed once.
    Query,
    /// A mutation executed once.
    Mutation,
    /// A subscription source and sequential event loop.
    Subscription,
}

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Validates, selects, and coerces one WebSocket operation exactly once.
    #[doc(hidden)]
    pub fn prepare_websocket_operation(
        &self,
        mut request: GraphQlRequest,
    ) -> Result<PreparedWebSocketOperation<C>, Box<GraphQlResponse>> {
        if request.request_telemetry().is_none() {
            request.telemetry = Some(self.inner.diagnostics.request_telemetry(None));
        }
        let prepared = self.prepare_request_plan(
            &request,
            PreparationPolicy::WebSocket,
            "websocket-operation",
            Ok,
        )?;
        let request = Arc::new(request);
        match prepared.kind {
            OperationType::Query | OperationType::Mutation => {
                Ok(PreparedWebSocketOperation::OneShot(Box::new(
                    PreparedOneShotOperation::from_plan(self.clone(), prepared, request),
                )))
            }
            OperationType::Subscription => Ok(PreparedWebSocketOperation::Subscription(Box::new(
                PreparedSubscription::from_plan(self.clone(), prepared, request)?,
            ))),
        }
    }
}
