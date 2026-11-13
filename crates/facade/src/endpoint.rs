//! Immutable endpoint handles and fail-closed construction.

mod build;
#[cfg(feature = "subscription-worker")]
mod managed;
mod request_identity;
mod stream_resources;
mod streaming;
mod subscription;
#[cfg(feature = "subscription-worker")]
mod subscription_runtime;
mod validation;
mod websocket;

use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;

use chigiri_http::{DeadlineFactory, RequestBody, ResponseParts, TransportRequest};
use chigiri_runtime::{ExecutableSchema, GraphQlRequest, GraphQlResponse, SchemaInfo};

use crate::draft::Draft;
use crate::transport::NeutralEndpoint;
use crate::{BoxFuture, EffectiveConfiguration, IntoEndpointInput};

/// Cheaply cloned handle sharing schema, HTTP lifecycle, and effective configuration.
pub struct Endpoint<C> {
    neutral: NeutralEndpoint<C>,
    schema: ExecutableSchema<C>,
    configuration: Arc<EffectiveConfiguration>,
    #[cfg(feature = "subscription-worker")]
    managed: Arc<crate::managed::Binding>,
}

impl<C> Clone for Endpoint<C> {
    fn clone(&self) -> Self {
        Self {
            neutral: self.neutral.clone(),
            schema: self.schema.clone(),
            configuration: Arc::clone(&self.configuration),
            #[cfg(feature = "subscription-worker")]
            managed: Arc::clone(&self.managed),
        }
    }
}

impl<C> Endpoint<C>
where
    C: Send + Sync + 'static,
{
    /// Creates the sole builder from generated resolvers or an already-built schema.
    ///
    /// The automatic request-ID seed is not acquired here; initialization is
    /// deferred to worker-local [`EndpointBuilder::build`]. This path does not
    /// perform transport-specific processing.
    pub fn builder<I>(input: I) -> MissingContextEndpointBuilder<C>
    where
        I: IntoEndpointInput<C>,
    {
        MissingContextEndpointBuilder {
            draft: Draft::new(input.into_endpoint_input()),
        }
    }

    /// Processes a transport-neutral request through the shared HTTP pipeline.
    pub fn handle<'a, B>(
        &'a self,
        request: TransportRequest<'a>,
        body: B,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, ResponseParts>
    where
        B: RequestBody + 'a,
    {
        self.neutral.handle(request, body, deadlines)
    }

    /// Executes directly with the same schema and execution deadline.
    pub fn execute<'a>(
        &'a self,
        request: GraphQlRequest,
        context: C,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, GraphQlResponse> {
        use chigiri_runtime::instrumentation::{InstrumentationKind, instrument_future};

        let existing = request
            .request_telemetry()
            .and_then(|telemetry| telemetry.instrumentation_scope())
            .cloned();
        let captured = existing
            .is_none()
            .then(|| self.schema.capture_instrumentation(&()))
            .flatten();
        let handoff = existing.or_else(|| {
            captured
                .as_ref()
                .map(|values| values.handoff_scope(InstrumentationKind::Request))
        });
        let duration = *self.configuration.execution_timeout().effective();
        let mut timeout = match &handoff {
            Some(scope) => scope.run(|| deadlines.sleep(duration)),
            None => deadlines.sleep(duration),
        };
        let mut execution = match &handoff {
            Some(scope) => scope.run(|| {
                self.schema
                    .execute_with_deadlines(request, context, deadlines)
            }),
            None => self
                .schema
                .execute_with_deadlines(request, context, deadlines),
        };
        instrument_future(
            handoff,
            poll_fn(move |task| {
                if timeout.as_mut().poll(task).is_ready() {
                    return Poll::Ready(crate::timeout::request_timeout_response());
                }
                execution.as_mut().poll(task)
            }),
        )
    }

    /// It returns the immutable schema identity and the generated ABI information.
    pub fn schema_info(&self) -> &SchemaInfo {
        self.schema.schema_info()
    }
    /// Validates an operation with the same schema configuration used by offline
    /// Catalog checks and benchmarks.
    pub fn validate_operation(
        &self,
        request: &GraphQlRequest,
    ) -> Result<chigiri_runtime::OperationIdentity, Box<GraphQlResponse>> {
        self.schema.validate_operation(request)
    }
    /// Builds adapter and diagnostic telemetry from the configured ID factories.
    pub fn request_telemetry(
        &self,
        trace_id: Option<chigiri_runtime::core::TraceId>,
    ) -> chigiri_runtime::RequestTelemetry {
        self.schema.request_telemetry(trace_id)
    }
    /// Returns the final redacted configuration snapshot applied to the endpoint.
    pub fn configuration(&self) -> &EffectiveConfiguration {
        &self.configuration
    }
    /// Returns UI response capability only when the endpoint explicitly enables it.
    ///
    /// Publishing the UI also exposes introspection for the same schema. The
    /// application applies policies such as authentication separately.
    #[cfg(feature = "graphiql")]
    pub fn graphiql(&self) -> Option<crate::graphiql::GraphiqlView> {
        self.configuration
            .graphiql_enabled()
            .effective()
            .then(crate::graphiql::GraphiqlView::enabled)
    }
    /// Returns the number of HTTP requests currently being processed.
    pub fn in_flight_requests(&self) -> usize {
        self.neutral.in_flight_requests()
    }
}

/// Context factory unconfigured type builder.
pub struct MissingContextEndpointBuilder<C> {
    pub(crate) draft: Draft<C>,
}

/// Endpoint builder with a configured context factory.
pub struct EndpointBuilder<C, S, F> {
    pub(crate) draft: Draft<C>,
    pub(crate) factory: F,
    pub(crate) marker: std::marker::PhantomData<fn() -> S>,
}

/// Capability marker for the state before request context is finalized.
///
/// Until `context_factory` or `without_context` is chosen,
/// `HttpComponents::response_hook` is unavailable inside `inject`.
pub struct ContextPending(std::marker::PhantomData<std::rc::Rc<()>>);
