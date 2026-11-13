//! Type-erasure boundary between the runtime-neutral executor and generated resolvers.

mod arguments;
mod collect;
mod complete;
mod context;
mod cost_budget;
mod cost_metadata;
mod directive;
mod engine;
mod execute;
mod field_dispatch;
mod fingerprint;
mod input_errors;
mod internal_error;
mod introspection;
mod lifecycle;
mod output;
mod output_bytes;
mod plugin;
mod selection;
mod subscription;
mod value;

pub use context::{FieldContext, OperationContext, RequestContext, SelectionMetadata};
#[doc(hidden)]
pub use cost_budget::CostMetadataLimit;
pub use cost_metadata::{CostArgumentMetadata, CostArgumentPresence, CostFieldMetadata};
#[doc(hidden)]
pub use cost_metadata::{collect_operation_cost_fields, collect_operation_cost_fields_bounded};
pub use directive::{ExecutableDirective, ExecutableDirectiveSite};
pub use engine::OutputLimitExceeded;
pub use execute::execute_operation;
#[doc(hidden)]
pub use execute::execute_prepared_operation;
pub use field_dispatch::{FieldDispatch, ObjectScope};
pub use fingerprint::{ResolverSchemaFingerprint, generated_resolver_abi_fingerprint};
#[doc(hidden)]
pub use internal_error::{ExecutionFailureKind, InternalErrorHandler};
pub use plugin::{FieldNext, OperationNext, Plugin, RequestNext};
#[doc(hidden)]
pub use subscription::{
    PreparedOperation, PreparedSubscriptionOperation, PreparedSubscriptionPlan,
    PreparedSubscriptionRoot, SubscriptionEventDispatch, SubscriptionExecutionInput,
    SubscriptionMetadataCharge, SubscriptionSetupDispatch, SubscriptionSetupOutcome,
    establish_subscription_source, execute_subscription_event, prepare_subscription_plan,
    prepare_subscription_root,
};
pub use value::{
    ArgumentValues, BoxFuture, DispatchAttempt, ResolvedObject, ResolvedValue, ResolverDispatch,
};

pub(crate) use collect::{CollectedField, collect_fields};
