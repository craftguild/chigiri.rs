//! Runtime-neutral endpoint context-factory and response-hook contracts.

use std::error::Error;
use std::fmt;

use chigiri_runtime::{GraphQlRequest, GraphQlResponse, Object};

use crate::{
    BoxFuture, RequestMethod, RequestPhase, ResponseHeaders, ResponseMediaType, TransportRequest,
};

/// Read-only decoded-request view passed to the context factory.
#[derive(Clone, Copy)]
pub struct RequestContext<'a> {
    transport: TransportRequest<'a>,
    request: &'a GraphQlRequest,
    method: RequestMethod,
    media: ResponseMediaType,
    raw_body: &'a [u8],
    phase: RequestPhase,
    connection_init: Option<&'a Object>,
}

impl<'a> RequestContext<'a> {
    /// Internal facade-pipeline entry point that creates a decoded request view.
    #[doc(hidden)]
    pub const fn __new(
        transport: TransportRequest<'a>,
        request: &'a GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
    ) -> Self {
        Self {
            transport,
            request,
            method,
            media,
            raw_body: &[],
            phase: RequestPhase::Finite,
            connection_init: None,
        }
    }

    /// Internal body bytes borrowed from decoding; GET requests use an empty slice.
    #[doc(hidden)]
    pub const fn __with_raw_body(mut self, raw_body: &'a [u8]) -> Self {
        if self.phase.retains_raw_body() {
            self.raw_body = raw_body;
        }
        self
    }

    /// Internal streaming entry point that installs one closed phase view.
    ///
    /// Non-finite phases never retain the decoded HTTP body. Initialization
    /// data is visible only to operation phases carried by WebSocket.
    #[doc(hidden)]
    pub const fn __with_phase(
        mut self,
        phase: RequestPhase,
        connection_init: Option<&'a Object>,
    ) -> Self {
        self.phase = phase;
        if !phase.retains_raw_body() {
            self.raw_body = &[];
        }
        self.connection_init = if phase.exposes_connection_init() {
            connection_init
        } else {
            None
        };
        self
    }

    /// Returns the exact body bytes before decoding.
    pub const fn raw_body(&self) -> &'a [u8] {
        self.raw_body
    }

    /// Returns the finite, setup, event, or WebSocket one-shot phase.
    pub const fn phase(&self) -> RequestPhase {
        self.phase
    }

    /// Returns authorized WebSocket initialization data for eligible phases.
    pub const fn connection_init(&self) -> Option<&'a Object> {
        self.connection_init
    }

    /// Returns current phase metadata, including its fresh request telemetry.
    pub const fn transport(&self) -> TransportRequest<'a> {
        self.transport
    }
    /// Returns the immutable decoded operation captured during admission.
    ///
    /// Its telemetry is the original operation snapshot. Streaming factories
    /// use `transport().request_telemetry()` for the current setup/event identity.
    pub const fn request(&self) -> &'a GraphQlRequest {
        self.request
    }
    pub const fn method(&self) -> RequestMethod {
        self.method
    }
    pub const fn response_media_type(&self) -> ResponseMediaType {
        self.media
    }
}

/// Keeps the execution context and response-hook state together.
pub struct PreparedContext<C, S> {
    execution_context: C,
    response_state: S,
    completion: Option<std::sync::Arc<dyn crate::ResponseCompletion>>,
}

impl<C, S> PreparedContext<C, S> {
    pub const fn new(execution_context: C, response_state: S) -> Self {
        Self {
            execution_context,
            response_state,
            completion: None,
        }
    }

    /// Associates state that is passed to the response hook after execution.
    pub fn with_response_completion(mut self, completion: impl crate::ResponseCompletion) -> Self {
        self.completion = Some(std::sync::Arc::new(completion));
        self
    }

    /// Internal facade-pipeline entry point that extracts context and hook state.
    #[doc(hidden)]
    pub fn __into_parts(self) -> (C, S, Option<std::sync::Arc<dyn crate::ResponseCompletion>>) {
        (self.execution_context, self.response_state, self.completion)
    }
}

impl<C> PreparedContext<C, ()> {
    pub const fn execution_only(execution_context: C) -> Self {
        Self::new(execution_context, ())
    }
}

/// Factory that creates an execution context for each request.
pub trait ContextFactory<C, S>: Send + Sync + 'static {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<C, S>, PrepareError>>;
}

impl<C, S, F> ContextFactory<C, S> for F
where
    F: Send
        + Sync
        + 'static
        + for<'a> Fn(RequestContext<'a>) -> BoxFuture<'a, Result<PreparedContext<C, S>, PrepareError>>,
{
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<C, S>, PrepareError>> {
        self(request)
    }
}

/// Read-only response-hook context available after execution.
pub struct ResponseHookContext<'a, S> {
    request: RequestContext<'a>,
    response: &'a GraphQlResponse,
    state: &'a S,
}

impl<'a, S> ResponseHookContext<'a, S> {
    /// Internal facade-pipeline entry point that creates the hook view.
    #[doc(hidden)]
    pub const fn __new(
        request: RequestContext<'a>,
        response: &'a GraphQlResponse,
        state: &'a S,
    ) -> Self {
        Self {
            request,
            response,
            state,
        }
    }

    pub const fn request_context(&self) -> RequestContext<'a> {
        self.request
    }
    pub const fn request(&self) -> &'a GraphQlRequest {
        self.request.request
    }
    pub const fn response(&self) -> &'a GraphQlResponse {
        self.response
    }
    pub const fn response_media_type(&self) -> ResponseMediaType {
        self.request.media
    }
    pub const fn state(&self) -> &'a S {
        self.state
    }
}

/// Application hook that can add response headers after execution.
pub trait ResponseHook<S>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        response: ResponseHookContext<'a, S>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>>;
}

impl<S, F> ResponseHook<S> for F
where
    F: Send
        + Sync
        + 'static
        + for<'a> Fn(ResponseHookContext<'a, S>) -> BoxFuture<'a, Result<ResponseHeaders, HookError>>,
{
    fn call<'a>(
        &'a self,
        response: ResponseHookContext<'a, S>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self(response)
    }
}

/// Endpoint-hook failure whose source is not exposed to the client.
pub struct HookError(Box<dyn Error + Send + Sync>);

impl HookError {
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Debug for HookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HookError")
    }
}

impl fmt::Display for HookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GraphQL endpoint hook failed")
    }
}

impl Error for HookError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// The context preparation phase failure classification.
pub enum PrepareError {
    Internal(HookError),
    Reject(crate::Rejection),
}

impl From<HookError> for PrepareError {
    fn from(error: HookError) -> Self {
        Self::Internal(error)
    }
}

impl From<crate::Rejection> for PrepareError {
    fn from(rejection: crate::Rejection) -> Self {
        Self::Reject(rejection)
    }
}
