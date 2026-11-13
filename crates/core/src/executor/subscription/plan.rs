//! Owned subscription-root metadata retained between setup and event execution.

use crate::{GraphQlResponse, Location, Object};

use super::super::{ArgumentValues, CostFieldMetadata, ExecutableDirective};
use super::PreparedSubscriptionOperation;

/// Context-free, fully coerced subscription metadata consumed by source setup.
#[doc(hidden)]
pub struct PreparedSubscriptionPlan {
    pub(super) root: PreparedSubscriptionRoot,
    pub(super) dispatch_arguments: ArgumentValues,
    pub(super) has_subselection: bool,
}

impl PreparedSubscriptionPlan {
    /// Returns the subscription root object type used for generated dispatch.
    pub fn parent_type(&self) -> &str {
        self.root.parent_type()
    }

    /// Returns the subscription-root schema field name.
    pub fn field_name(&self) -> &str {
        self.root.field_name()
    }

    /// Returns the response key after alias expansion.
    pub fn response_key(&self) -> &str {
        self.root.response_key()
    }

    /// Returns the pre-context, coerced public arguments.
    pub const fn arguments(&self) -> &Object {
        self.root.arguments()
    }

    /// Returns the prepared root field source locations.
    pub fn locations(&self) -> &[Location] {
        self.root.locations()
    }
}

/// Owned, public subscription-root metadata that is safe to retain with a source.
#[doc(hidden)]
pub struct PreparedSubscriptionRoot {
    pub(super) operation: PreparedSubscriptionOperation,
    pub(super) parent_type: String,
    pub(super) field_name: String,
    pub(super) response_key: String,
    pub(super) locations: Vec<Location>,
    pub(super) arguments: Object,
    pub(super) directives: Vec<ExecutableDirective>,
    pub(super) operation_directives: Vec<ExecutableDirective>,
    pub(super) cost_fields: Vec<CostFieldMetadata>,
}

impl PreparedSubscriptionRoot {
    /// Returns the subscription root object type used for generated dispatch.
    pub fn parent_type(&self) -> &str {
        &self.parent_type
    }

    /// Returns the response key after alias expansion.
    pub fn response_key(&self) -> &str {
        &self.response_key
    }

    /// Returns the subscription-root schema field name.
    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    /// Returns the setup-coerced public arguments.
    pub const fn arguments(&self) -> &Object {
        &self.arguments
    }

    /// Returns the prepared root field source locations.
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }
}

/// Either an application-owned source session or a terminal setup response.
#[doc(hidden)]
pub enum SubscriptionSetupOutcome<S> {
    /// Setup reached the generated source terminal exactly once.
    Ready {
        root: Box<PreparedSubscriptionRoot>,
        session: S,
    },
    /// Coercion, policy, middleware, or resolver setup rejected the operation.
    Response(GraphQlResponse),
}
