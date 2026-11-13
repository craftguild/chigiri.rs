//! Immutable operation package shared by deferred execution paths.

use std::sync::Arc;

use crate::executable::{
    CoercedVariables, CustomScalarRegistry, OperationSelectionError, SelectedOperation,
    ValidatedDocument,
};
use crate::schema::SemanticSchema;
use crate::{ExecutionLimits, OperationIdentity, Source};

/// Exact validated operation state retained across a resource-owner handoff.
#[derive(Clone)]
#[doc(hidden)]
pub struct PreparedOperation {
    pub(in crate::executor) schema: Arc<SemanticSchema>,
    pub(in crate::executor) document: Arc<ValidatedDocument>,
    pub(in crate::executor) operation_name: Option<String>,
    pub(in crate::executor) variables: Arc<CoercedVariables>,
    pub(in crate::executor) scalars: Arc<CustomScalarRegistry>,
    pub(in crate::executor) source: Arc<Source>,
    pub(in crate::executor) limits: ExecutionLimits,
    pub(in crate::executor) introspection_enabled: bool,
    pub(in crate::executor) identity: Arc<OperationIdentity>,
    pub(in crate::executor) catalog_cost_hint: Option<u64>,
}

impl PreparedOperation {
    /// Binds immutable schema, document, selection, variables, and source metadata once.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: Arc<SemanticSchema>,
        document: Arc<ValidatedDocument>,
        operation_name: Option<String>,
        variables: Arc<CoercedVariables>,
        scalars: Arc<CustomScalarRegistry>,
        source: Arc<Source>,
        limits: ExecutionLimits,
        introspection_enabled: bool,
        identity: Arc<OperationIdentity>,
        catalog_cost_hint: Option<u64>,
    ) -> Result<Self, OperationSelectionError> {
        document.select_operation(operation_name.as_deref())?;
        Ok(Self {
            schema,
            document,
            operation_name,
            variables,
            scalars,
            source,
            limits,
            introspection_enabled,
            identity,
            catalog_cost_hint,
        })
    }

    pub(in crate::executor) fn selected(
        &self,
    ) -> Result<SelectedOperation<'_>, OperationSelectionError> {
        self.document
            .select_operation(self.operation_name.as_deref())
    }
}

/// Compatibility name for the package retained by subscription execution.
#[doc(hidden)]
pub type PreparedSubscriptionOperation = PreparedOperation;
