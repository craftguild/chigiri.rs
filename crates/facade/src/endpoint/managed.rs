//! Common SDK activation and response boundary for official native bridges.

use std::sync::Arc;

use chigiri_http::{BoxFuture, DeadlineFactory, RequestBody, TransportRequest};

use super::Endpoint;
use crate::managed::{
    Control, ManagedHttpRequest, ManagedHttpResponse, ManagedService, ManagedServiceError,
};

impl<C: Send + Sync + 'static> Endpoint<C> {
    pub(crate) fn managed_capture_instrumentation(
        &self,
        request: TransportRequest<'_>,
    ) -> Option<chigiri_runtime::instrumentation::CapturedInstrumentation> {
        request
            .__instrumentation_capture()
            .cloned()
            .or_else(|| self.schema.capture_instrumentation(&request))
    }

    pub(crate) fn managed_websocket_state(&self) -> Arc<crate::transport::EndpointState<C>> {
        self.neutral.shared_state()
    }

    pub(crate) fn managed_http_fault_reporter(&self) -> crate::managed::HttpFaultReporter {
        crate::managed::HttpFaultReporter::new(self.neutral.shared_state().schema.clone())
    }

    pub(crate) fn managed_ingress<'a, B: RequestBody + 'a>(
        &self,
        request: TransportRequest<'a>,
        body: B,
        deadlines: &'static dyn DeadlineFactory,
        monitor: crate::transport::deadline::HttpDeadlineMonitor,
    ) -> BoxFuture<'a, crate::transport::streaming::SubscriptionHttpIngress<'static, C>> {
        crate::transport::streaming::handle_monitored(
            self.neutral.shared_state(),
            request,
            body,
            deadlines,
            monitor,
        )
    }

    /// Creates an optional SDK stop hook for the application's native server.
    ///
    /// After `signal` completes, new GraphQL admission closes. Immediate
    /// requests cancellation and returns without waiting for worker completion.
    /// Graceful stops source demand and waits for eligible work within one
    /// monotonic deadline, completing on drain, expiry or infrastructure failure.
    /// Inspect [`Self::shutdown_outcome`] when the classification is needed.
    ///
    /// Pass this future to the native server's shutdown API. The SDK finishes
    /// its selected stop policy before that future permits native server
    /// shutdown to begin. Other routes and TCP admission may remain available
    /// during SDK drain. The duration bounds SDK waiting rather than subsequent
    /// server shutdown, arbitrary Rust destructors or delivery to the client.
    ///
    /// The application owns its signal, listener and runtime. Registering a
    /// service without this hook is supported; releasing its last service owner
    /// requests cancellation without detecting when native server shutdown began.
    pub fn shutdown_on<F>(
        &self,
        signal: F,
        options: crate::ShutdownOptions,
    ) -> BoxFuture<'static, ()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.managed.control.shutdown_on(signal, options)
    }
    /// Activates the endpoint-shared SDK owner in the existing runtime.
    #[doc(hidden)]
    pub fn __managed_service(&self) -> Result<ManagedService, ManagedServiceError> {
        let websocket_capacity = self
            .configuration()
            .subscription_protocol()
            .effective()
            .max_websocket_connections;
        let service = self.managed.activate(websocket_capacity, || {
            let guard = self
                .__register_subscription_runtime()
                .map_err(|_| ManagedServiceError::Infrastructure)?;
            let (registration, driver) = self
                .__http_streaming_supervision()
                .map_err(|_| ManagedServiceError::Capacity)?;
            Ok((registration, driver, guard))
        })?;
        let _ = self
            .neutral
            .shared_state()
            .subscription_execution
            .managed
            .set(Arc::downgrade(&self.managed.control));
        Ok(service)
    }

    /// Executes one admitted native request with SDK cancellation and completion.
    #[doc(hidden)]
    pub fn __handle_managed_http<B: RequestBody + 'static>(
        &self,
        scope: ManagedHttpRequest,
        request: TransportRequest<'_>,
        body: B,
        version: crate::__streaming::WebSocketHttpVersion,
        deadlines: &'static dyn DeadlineFactory,
    ) -> BoxFuture<'static, ManagedHttpResponse> {
        scope.start(self, request, body, version, deadlines)
    }

    /// Transfers an SDK-issued charged upgrade and its native socket conversion.
    /// This closed bridge role is not an application future/task registry.
    #[doc(hidden)]
    pub fn __attach_managed_websocket<S: crate::__streaming::WebSocketIo>(
        &self,
        upgrade: crate::managed::ManagedWebSocketUpgrade,
        socket: BoxFuture<'static, Result<S, crate::__streaming::WebSocketIoError>>,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<(), ManagedServiceError> {
        upgrade.attach(self, socket, deadlines)
    }

    /// Inspects the first selected managed shutdown outcome without waiting.
    ///
    /// Cancellation requested does not certify worker completion or client
    /// receipt. Late physical completion never rewrites an existing outcome.
    pub fn shutdown_outcome(&self) -> Option<crate::ShutdownOutcome> {
        self.managed.control.outcome()
    }

    pub(crate) fn managed_control(&self) -> Arc<Control> {
        Arc::clone(&self.managed.control)
    }
}
