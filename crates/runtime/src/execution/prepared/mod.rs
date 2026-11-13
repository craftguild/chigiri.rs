//! Shared fully validated operation checkpoints.

mod metadata;
mod one_shot;
mod prepare;
mod run;
mod websocket;

use std::sync::Arc;

use chigiri_core::OperationIdentity;
use chigiri_core::executable::InputCoercionBudget;
use chigiri_core::executor::PreparedOperation;
use chigiri_core::language::ast::OperationType;

pub use one_shot::{OneShotOperationKind, PreparedOneShotOperation};
pub use websocket::{PreparedWebSocketOperation, WebSocketOperationKind};

pub(crate) use metadata::retained_request_metadata_bytes;
pub(crate) use prepare::PreparationPolicy;

/// Fully validated and coerced state with its remaining request-wide budget.
pub(crate) struct PreparedRequestPlan {
    pub(crate) operation: PreparedOperation,
    pub(crate) identity: Arc<OperationIdentity>,
    pub(crate) input_budget: Option<InputCoercionBudget>,
    pub(crate) kind: OperationType,
}
