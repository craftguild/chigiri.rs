//! Common finite/streaming preparation and safe pre-head failure mapping.

mod input;
mod observation;
pub(crate) use observation::HttpFaultReporter;

use std::sync::Arc;

use chigiri_http::{DeadlineFactory, RequestBody, ResponseParts, TransportRequest};
use chigiri_runtime::instrumentation::{
    CapturedInstrumentation, InstrumentationKind, InstrumentationScope,
};

use super::Control;
use super::control::RequestPermit;
use super::sockets::{ManagedWebSocketUpgrade, SocketRegistry};
use crate::__streaming::{WebSocketHttpVersion, WebSocketUpgradeIngress};
use crate::Endpoint;
use crate::transport::streaming::supervisor::{HttpStreamingRegistration, SupervisedHttpResponse};
use crate::transport::streaming::{
    PreparedHttpSubscription, SubscriptionHttpIngress, SubscriptionInitialResponse,
};

/// Internal SDK request scope, without a service-owner lease.
#[doc(hidden)]
pub struct ManagedHttpRequest {
    permit: RequestPermit,
    control: Arc<Control>,
    registration: HttpStreamingRegistration,
    sockets: Arc<SocketRegistry>,
    requests: Arc<super::requests::RequestRegistry>,
    started: tokio::time::Instant,
}

impl ManagedHttpRequest {
    pub(crate) fn new(
        permit: RequestPermit,
        control: Arc<Control>,
        registration: HttpStreamingRegistration,
        sockets: Arc<SocketRegistry>,
        requests: Arc<super::requests::RequestRegistry>,
    ) -> Self {
        Self {
            permit,
            control,
            registration,
            sockets,
            requests,
            started: tokio::time::Instant::now(),
        }
    }

    pub(crate) fn start<C: Send + Sync + 'static, B: RequestBody + 'static>(
        self,
        endpoint: &Endpoint<C>,
        request: TransportRequest<'_>,
        body: B,
        version: WebSocketHttpVersion,
        deadlines: &'static dyn DeadlineFactory,
    ) -> crate::BoxFuture<'static, ManagedHttpResponse> {
        let captured = endpoint.managed_capture_instrumentation(request);
        let scope = captured
            .as_ref()
            .map(|value| value.handoff_scope(InstrumentationKind::Request));
        match scope {
            Some(scope) => scope
                .run(|| self.start_captured(endpoint, request, body, version, deadlines, captured)),
            None => self.start_captured(endpoint, request, body, version, deadlines, None),
        }
    }

    fn start_captured<C: Send + Sync + 'static, B: RequestBody + 'static>(
        self,
        endpoint: &Endpoint<C>,
        request: TransportRequest<'_>,
        body: B,
        version: WebSocketHttpVersion,
        deadlines: &'static dyn DeadlineFactory,
        captured: Option<CapturedInstrumentation>,
    ) -> crate::BoxFuture<'static, ManagedHttpResponse> {
        let Self {
            permit,
            control,
            registration,
            sockets,
            requests,
            started,
        } = self;
        if !Arc::ptr_eq(&control, &endpoint.managed_control()) {
            return Box::pin(async { ManagedHttpResponse::internal_failure() });
        }
        let media = chigiri_http::negotiate_response_media(request.headers())
            .unwrap_or(chigiri_http::ResponseMediaType::GraphQlResponseJson);
        let work = HttpWork {
            endpoint: endpoint.clone(),
            input: input::Input::capture(request, captured),
            body,
            control,
            registration,
            sockets,
            version,
            deadlines,
        };
        requests.start(
            work,
            permit,
            started,
            *endpoint.configuration().overall_timeout().effective(),
            media,
        )
    }
}

pub(crate) struct HttpWork<C, B> {
    endpoint: Endpoint<C>,
    input: input::Input,
    body: B,
    control: Arc<Control>,
    registration: HttpStreamingRegistration,
    sockets: Arc<SocketRegistry>,
    version: WebSocketHttpVersion,
    deadlines: &'static dyn DeadlineFactory,
}

impl<C: Send + Sync + 'static, B: RequestBody + 'static> HttpWork<C, B> {
    pub(crate) fn instrumentation_capture(&self) -> Option<CapturedInstrumentation> {
        self.input.instrumentation().cloned()
    }

    pub(crate) fn handoff_scope(&self) -> Option<InstrumentationScope> {
        self.input
            .instrumentation()
            .map(|value| value.handoff_scope(InstrumentationKind::Request))
    }

    pub(crate) fn fault_reporter(&self) -> HttpFaultReporter {
        self.endpoint.managed_http_fault_reporter()
    }

    pub(crate) fn deadline_factory(&self) -> &'static dyn DeadlineFactory {
        self.deadlines
    }

    pub(crate) async fn run(
        self,
        monitor: crate::transport::deadline::HttpDeadlineMonitor,
    ) -> ManagedHttpResponse {
        let Self {
            endpoint,
            input,
            body,
            control,
            registration,
            sockets,
            version,
            deadlines,
        } = self;
        let headers = input.headers();
        let request = input.view(&headers);
        let endpoint = &endpoint;
        {
            let execution = async {
                if websocket_candidate(request) {
                    drop(body);
                    if control.is_stopping() {
                        return ManagedHttpResponse::unavailable();
                    }
                    return match endpoint
                        .__admit_websocket_upgrade(request, version, deadlines)
                        .await
                    {
                        WebSocketUpgradeIngress::Finite(response) => {
                            ManagedHttpResponse::Finite(response)
                        }
                        WebSocketUpgradeIngress::Admitted(seed) => {
                            match endpoint.__claim_websocket_upgrade(seed) {
                                Ok(seed) => ManagedHttpResponse::Upgrade(Box::new(
                                    ManagedWebSocketUpgrade::new(seed, sockets),
                                )),
                                Err(_) => ManagedHttpResponse::internal_failure(),
                            }
                        }
                    };
                }
                match endpoint
                    .managed_ingress(request, body, deadlines, monitor)
                    .await
                {
                    SubscriptionHttpIngress::Finite(response) => {
                        ManagedHttpResponse::Finite(response)
                    }
                    SubscriptionHttpIngress::Pending(pending) => {
                        tokio::select! {
                            biased;
                            _ = control.source_stopped() => ManagedHttpResponse::unavailable(),
                            preparation = pending.prepare() => prepared(preparation, &registration, &control),
                        }
                    }
                }
            };
            tokio::select! {
                biased;
                _ = control.cancelled() => ManagedHttpResponse::empty(
                    if control.outcome() == Some(crate::ShutdownOutcome::InfrastructureFailure) { 500 } else { 503 }
                ),
                response = execution => response,
            }
        }
    }
}

fn websocket_candidate(request: TransportRequest<'_>) -> bool {
    request.headers().iter().any(|header| {
        header.name.eq_ignore_ascii_case("upgrade")
            || header
                .name
                .as_bytes()
                .get(..14)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"sec-websocket-"))
            || (header.name.eq_ignore_ascii_case("connection")
                && header
                    .value
                    .split(|byte| *byte == b',')
                    .any(|token| token.trim_ascii().eq_ignore_ascii_case(b"upgrade")))
    })
}

fn prepared(
    prepared: PreparedHttpSubscription,
    registration: &HttpStreamingRegistration,
    control: &Control,
) -> ManagedHttpResponse {
    match prepared {
        PreparedHttpSubscription::Failed(_) => ManagedHttpResponse::empty(500),
        PreparedHttpSubscription::Response(response) => ManagedHttpResponse::Prepared(response),
        PreparedHttpSubscription::Ready(ready) => {
            let result = registration.attach_managed(ready, control);
            match result {
                Ok(response) => ManagedHttpResponse::Streaming(response),
                Err(_) => ManagedHttpResponse::empty(500),
            }
        }
    }
}

/// Closed SDK response variants; adapters only convert native response types.
#[doc(hidden)]
pub enum ManagedHttpResponse {
    /// Same-endpoint admitted upgrade awaiting native socket conversion.
    Upgrade(Box<ManagedWebSocketUpgrade>),
    /// An ordinary finite response with unchanged lifecycle and status.
    Finite(ResponseParts),
    /// An already charged initial finite/profile error response.
    Prepared(SubscriptionInitialResponse),
    /// A supervised stream retaining the original physical byte/permit owners.
    Streaming(SupervisedHttpResponse),
    /// A bodyless unavailable/internal response, with no encoded fallback.
    Empty {
        /// Safe HTTP status selected by the SDK.
        status: u16,
    },
}

impl ManagedHttpResponse {
    /// Returns the allocation-free masked pre-head infrastructure failure.
    pub const fn internal_failure() -> Self {
        Self::empty(500)
    }
    /// Returns the allocation-free unavailable response for closed admission.
    pub const fn unavailable() -> Self {
        Self::empty(503)
    }

    const fn empty(status: u16) -> Self {
        Self::Empty { status }
    }
}
