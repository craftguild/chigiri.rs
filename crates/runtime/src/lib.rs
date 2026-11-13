//! Façade crate that provides a stable public API for application and generated code.

mod cache;
mod diagnostics;
mod dispatch;
mod enum_binding;
mod execution;
mod generated;
pub mod instrumentation;
mod operation_metadata;
mod owned_schema;
mod runtime_schema;
mod schema;
mod schema_info;
mod schema_reconfigure;
mod subscription;
#[doc(hidden)]
pub mod __private {
    pub use crate::private::*;
}
mod private;

pub use chigiri_core as core;
pub use chigiri_core::executable::ScalarCoercionError;
pub use chigiri_core::executor::{
    CostArgumentMetadata, CostArgumentPresence, CostFieldMetadata, ExecutableDirective,
    ExecutableDirectiveSite, FieldNext, OperationContext, OperationNext, Plugin, RequestContext,
    RequestNext, SelectionMetadata,
};
pub use chigiri_core::{
    DigestParseError, DocumentDigest, ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError,
    ErrorObserver, ExecutionLimits, FieldError, FieldResult, GraphQlError, GraphQlRequest,
    GraphQlResponse, Id, IdentityError, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult, Location, Object, OperationIdentity, OperationKey, OperationName,
    OperationNameError, PathSegment, RequestId, RequestIdFactory, RequestIdFactoryError,
    RequestIdentity, RequestTelemetry, ResultShapeDigest, Sampler, SamplingDecision, SchemaDigest,
    TraceId, Value,
};
pub use enum_binding::EnumBindingError;
pub use operation_metadata::ValidatedOperation;
pub use owned_schema::{OwnedResolverDispatch, OwnedSchema, OwnedSchemaError, OwnedSchemaInput};
pub use schema_info::{SchemaInfo, SchemaSourceKind};
pub use subscription::{SubscriptionOverflow, SubscriptionSource};

pub use schema::{
    DocumentCacheLimits, ExecutableSchema, ExecutableSchemaBuilder, IntrospectionPolicy,
    SchemaBuildError,
};
#[doc(hidden)]
pub use schema_reconfigure::ExecutableSchemaRebuilder;

pub use generated::{
    AppliedDirectiveDescriptor, BuiltInScalarOverlayDescriptor, ConstObjectFieldDescriptor,
    ConstValueDescriptor, DescriptorList, DescriptorListIter, DirectiveArgumentDescriptor,
    DirectiveDefinitionDescriptor, EnumValueDescriptor, FieldContext, FieldDescriptor,
    GENERATED_ABI_VERSION, Input, InputValueDescriptor, OperationRootsDescriptor, Scalar,
    SchemaDescriptor, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor,
};
