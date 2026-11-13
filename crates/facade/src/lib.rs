#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! The only public facade where an application builds an endpoint.

mod build_support;
mod builder;
mod component_ops;
mod components;
mod configuration;
mod draft;
mod endpoint;
#[cfg(feature = "graphiql")]
pub mod graphiql;
mod input;
#[cfg(feature = "subscription-worker")]
mod managed;
pub mod rate;
mod shared;
mod shutdown;
mod subscription;
mod timeout;
mod transport;

pub use build_support::{
    ConfigurationConstraint, ConfigurationDiagnostic, ConfigurationOperand, ConfigurationSlot,
    ConfigurationValue, ConfigurationViolation, EndpointBuildError,
};
pub use chigiri_http::{
    AcceptConnectionInit, BodyReadError, BoxFuture, ConnectionInitAuthorizer,
    ConnectionInitContext, ConnectionInitDecision, ConnectionInitError, ContextFactory,
    DeadlineFactory, HookError, PrepareError, PreparedContext, RequestBody,
    RequestContext as HttpRequestContext, RequestMethod, RequestPhase, ResponseCompletion,
    ResponseCompletionOutcome, ResponseHeader, ResponseHeaderError, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseMediaType, ResponseParts, SubscriptionTransport, TransportRequest,
    WebSocketOneShot,
};
pub use components::{
    Components, ExecutionComponents, HttpComponents, ObservabilityComponents, OperationComponents,
    PluginComponents, RateLimitComponents, SubscriptionComponents,
};
pub use configuration::{
    ConfigurationOrigin, EffectiveConfiguration, RateLimitCoverage, RateLimitStageConfiguration,
    SafetyOverride, SettingSnapshot,
};
pub use endpoint::{ContextPending, Endpoint, EndpointBuilder, MissingContextEndpointBuilder};
pub use input::{EndpointInput, IntoEndpointInput};
#[cfg(feature = "subscription-worker")]
pub use managed::ManagedServiceError;
pub use shutdown::{ShutdownMode, ShutdownOptions, ShutdownOptionsError, ShutdownOutcome};
pub use subscription::settings::{
    ConnectionInitAuthorization, SubscriptionLimits, SubscriptionProtocolSettings,
};

/// Internal lifecycle ownership used by the official streaming bridges.
#[doc(hidden)]
pub mod __streaming {
    #[cfg(feature = "subscription-worker")]
    pub use crate::managed::{
        ManagedHttpRequest, ManagedHttpResponse, ManagedService, ManagedWebSocketUpgrade,
    };
    pub use crate::subscription::bridge_resources::{
        ActiveBridgeOperation, BridgeCancellation, BridgeCleanupCoordinator, BridgeCleanupDriver,
        BridgeCleanupError, BridgeCleanupOutcome, BridgeConnection, BridgeConnectionAdmission,
        BridgeEmissionLease, BridgeGeneration, BridgeOperationKind, BridgeQuarantineFailure,
        BridgeResourceError, DetachedCleanup, HttpSubscriptionPhysicalLease,
        PendingBridgeOperation, TerminalBridgeConnection, TerminalBridgeOperation,
        WebSocketConnectionPhysicalLease, WebSocketOperationPhysicalLease,
    };
    pub use crate::subscription::control::{
        HttpHeartbeatStaging, HttpSubscriptionTerminal, HttpTerminalStaging,
        SubscriptionControlFrame, SubscriptionControlSignal, WebSocketControlClass,
        WebSocketControlStaging, WebSocketWriterSchedule, WebSocketWriterTurn,
    };
    pub use crate::subscription::driver::{
        EndpointSubscription, SubscriptionCompletion, SubscriptionInitialization,
        SubscriptionOutput,
    };
    pub use crate::subscription::outbox::{
        SubscriptionEventReservation, SubscriptionOutbox, SubscriptionOutboxSignal,
        SubscriptionSendFrame, SubscriptionSendProgress,
    };
    #[cfg(feature = "subscription-worker")]
    pub use crate::subscription::worker::{
        SubscriptionProcessShutdown, SubscriptionRuntimeGuard, SubscriptionRuntimeShutdown,
        SubscriptionWorkerRuntimeError, shutdown_subscription_runtime_process,
    };
    #[cfg(feature = "subscription-worker")]
    pub use crate::transport::streaming::supervisor::{
        AcceptedHttpStreamingFrame, HttpStreamingBody, HttpStreamingDriver, HttpStreamingError,
        HttpStreamingFrame, HttpStreamingRegistration, SupervisedHttpResponse,
    };
    #[cfg(feature = "subscription-worker")]
    pub use crate::transport::streaming::{
        HttpSubscriptionEvents, HttpSubscriptionPreparation, HttpSubscriptionSignal,
        PreparedHttpSubscription, ReadyHttpSubscription, SubscriptionInitialResponse,
    };
    pub use crate::transport::streaming::{PendingHttpSubscription, SubscriptionHttpIngress};
    #[cfg(feature = "subscription-worker")]
    pub use crate::transport::websocket::{
        AckPendingWebSocketConnection, InitializedWebSocketConnection, InitializedWebSocketInput,
        WebSocketAcknowledgementFrame, WebSocketAcknowledgementProgress,
        WebSocketAcknowledgementResolution, WebSocketConnectionCleanup, WebSocketConnectionClose,
        WebSocketConnectionFrame, WebSocketConnectionHandoff, WebSocketConnectionHandoffResolution,
        WebSocketConnectionInitialization, WebSocketConnectionResolution, WebSocketInbound,
        WebSocketInitializationInput, WebSocketIo, WebSocketIoError, WebSocketOpeningDriver,
        WebSocketOpeningOutcome, WebSocketSessionDriver, WebSocketWireFrame,
        WebSocketWriteProgress,
    };
    pub use crate::transport::websocket::{
        AdmittedWebSocketUpgrade, ClaimedWebSocketUpgrade, WebSocketSeedError,
        WebSocketUpgradeIngress,
    };
    pub use chigiri_http::WebSocketHttpVersion;
}

#[doc(hidden)]
pub use chigiri_runtime::__private;
pub use chigiri_runtime::core;
pub use chigiri_runtime::core::{
    GENERATOR_INFO_SCHEMA, GeneratorBuild, GeneratorInfo, GeneratorInfoError,
};
pub use chigiri_runtime::{
    AppliedDirectiveDescriptor, BuiltInScalarOverlayDescriptor, ConstObjectFieldDescriptor,
    ConstValueDescriptor, CostArgumentMetadata, CostArgumentPresence, CostFieldMetadata,
    DescriptorList, DescriptorListIter, DigestParseError, DirectiveArgumentDescriptor,
    DirectiveDefinitionDescriptor, DocumentCacheLimits, DocumentDigest, EnumBindingError,
    EnumValueDescriptor, ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver,
    ExecutableDirective, ExecutableDirectiveSite, ExecutionLimits, FieldContext, FieldDescriptor,
    FieldError, FieldNext, FieldResult, GENERATED_ABI_VERSION, GraphQlError, GraphQlRequest,
    GraphQlResponse, Id, IdentityError, Input, InputValueDescriptor, IntrospectionPolicy,
    LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult, Location, Object,
    OperationContext, OperationIdentity, OperationKey, OperationName, OperationNameError,
    OperationNext, OperationRootsDescriptor, PathSegment, Plugin, RequestContext, RequestId,
    RequestIdFactory, RequestIdFactoryError, RequestIdentity, RequestNext, RequestTelemetry,
    ResultShapeDigest, Sampler, SamplingDecision, Scalar, ScalarCoercionError, SchemaBuildError,
    SchemaDescriptor, SchemaDigest, SchemaInfo, SchemaSourceKind, SelectionMetadata,
    SubscriptionOverflow, SubscriptionSource, TraceId, TypeDescriptor, TypeDescriptorKind,
    TypeRefDescriptor, ValidatedOperation, Value,
};

/// Neutral contract that only uses custom bridge and provider implementations.
pub mod spi {
    pub use chigiri_http::{
        AdmissionDecision, AdmissionError, ApplicationMetadata, BodyReadError, BoxFuture,
        BufferedBody, CsrfProtection, DeadlineFactory, DecodedRequestHook, DecodedRequestView,
        HttpLifecycle, MetadataError, MetadataField, OperationSourceResolver, Rejection,
        RejectionError, RequestBody, RequestEnvelope, RequestHeader, RequestMethod,
        ResolvedOperationSource, ResponseHeader, ResponseHeaderError, ResponseHeaders,
        ResponseMediaType, ResponseParts, RetryAfter, SourceResolutionError, TransportAdmission,
        TransportRequest, TransportUri,
    };
    pub use chigiri_runtime::instrumentation::{
        ExecutionScope, Instrumentation, InstrumentationGuard, InstrumentationKind,
        InstrumentationScope, PropagationCarrier, PropagationContext, PropagationField,
        TraceParent,
    };
    pub use chigiri_runtime::{ExecutableSchema, ExecutableSchemaBuilder};
}

/// The user typically uses a generated resolver.
pub mod experimental {
    pub use chigiri_runtime::{
        OwnedResolverDispatch, OwnedSchema, OwnedSchemaError, OwnedSchemaInput,
    };
}
