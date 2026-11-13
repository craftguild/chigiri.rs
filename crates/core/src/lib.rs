#![forbid(unsafe_code)]
//! Dependency-free GraphQL language processing and execution core.

pub mod diagnostic;
pub mod executable;
pub mod executor;
pub mod instrumentation;
pub mod language;
pub mod schema;
pub mod source;
pub mod value;

mod deadline;
mod execution_limits;
mod field_error;
mod generator;
mod identity;
mod numeric;
mod observe;
mod request;
mod response;
mod telemetry;

pub use deadline::DeadlineFactory;
pub use diagnostic::{Diagnostic, DiagnosticBag, DiagnosticCode};
pub use execution_limits::ExecutionLimits;
pub use field_error::{FieldError, FieldResult};
pub use generator::{GENERATOR_INFO_SCHEMA, GeneratorBuild, GeneratorInfo, GeneratorInfoError};
pub use identity::{
    DigestParseError, DocumentDigest, OperationIdentity, OperationKey, OperationName,
    OperationNameError, ResultShapeDigest, SchemaDigest,
};
pub use observe::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};
pub use request::GraphQlRequest;
pub use response::{GraphQlError, GraphQlResponse, Location, PathSegment};
pub use source::{Source, SourceId, SourceMap, Span};
pub use telemetry::{
    ErrorId, ErrorIdFactory, ErrorIdFactoryError, IdentityError, RequestId, RequestIdFactory,
    RequestIdFactoryError, RequestIdentity, RequestTelemetry, Sampler, SamplingDecision, TraceId,
};
pub use value::{Id, Object, Value};

#[cfg(test)]
mod public_api_tests;
