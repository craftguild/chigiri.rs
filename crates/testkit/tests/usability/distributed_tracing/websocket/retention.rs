//! Optional opening correlation degrades before copying without keeping a span.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri::__streaming::{ClaimedWebSocketUpgrade, WebSocketUpgradeIngress};
use chigiri::spi::{ApplicationMetadata, RequestHeader};
use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt, TraceState};

use super::super::super::support::{NeverDeadline, block_on};
use super::super::{PrivateValue, parent, query, recording};

struct Identity;
impl chigiri::RequestIdFactory for Identity {
    fn create(&self) -> Result<chigiri::RequestId, chigiri::RequestIdFactoryError> {
        Ok(chigiri::RequestId::new("opening-request").unwrap())
    }
}

fn endpoint(tracer: Option<recording::CountedTracer>, limit: usize) -> chigiri::Endpoint<()> {
    chigiri::Endpoint::builder(query::resolvers())
        .inject(|components| {
            components.observability().request_id_factory(Identity);
            if let Some(tracer) = tracer {
                components
                    .observability()
                    .instrumentation(OpenTelemetryInstrumentation::new(tracer));
            }
        })
        .subscription_limits(chigiri::SubscriptionLimits {
            max_retained_metadata_bytes: limit,
            ..Default::default()
        })
        .without_context()
        .build()
        .unwrap()
}

fn admit(endpoint: &chigiri::Endpoint<()>, suppressed: bool) -> WebSocketUpgradeIngress<'static> {
    admit_with_legacy(endpoint, suppressed, false)
}

fn admit_with_legacy(
    endpoint: &chigiri::Endpoint<()>,
    suppressed: bool,
    legacy: bool,
) -> WebSocketUpgradeIngress<'static> {
    let padding = [b'x'; 1024];
    let headers = [
        RequestHeader::new("host", b"example.test"),
        RequestHeader::new("connection", b"upgrade"),
        RequestHeader::new("upgrade", b"websocket"),
        RequestHeader::new("sec-websocket-version", b"13"),
        RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
        RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
        RequestHeader::new("x-retained-padding", &padding),
    ];
    let metadata = ApplicationMetadata::default();
    let trace = chigiri::TraceId::new("legacy").unwrap();
    let mut request =
        chigiri::TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    if legacy {
        request = request.with_trace_id(&trace);
    }
    let future = {
        let selected = parent(8);
        let selected = SpanContext::new(
            selected.trace_id(),
            selected.span_id(),
            selected.trace_flags(),
            true,
            TraceState::from_key_value([("vendor", "value")]).unwrap(),
        );
        let context = Context::new()
            .with_remote_span_context(selected)
            .with_value(PrivateValue);
        let context = if suppressed {
            context.with_telemetry_suppressed()
        } else {
            context
        };
        let _guard = context.attach();
        let deadlines: &'static NeverDeadline = &NeverDeadline;
        endpoint.__admit_websocket_upgrade(
            request,
            chigiri_http::WebSocketHttpVersion::Http11,
            deadlines,
        )
    };
    std::thread::scope(|threads| {
        threads
            .spawn(move || {
                let worker = Context::new()
                    .with_remote_span_context(parent(9))
                    .with_value(PrivateValue);
                let worker = if suppressed {
                    worker
                } else {
                    worker.with_telemetry_suppressed()
                };
                let _guard = worker.attach();
                let result = block_on(future);
                assert_eq!(*Context::current().span().span_context(), parent(9));
                assert_eq!(Context::current().is_telemetry_suppressed(), !suppressed);
                assert!(Context::current().get::<PrivateValue>().is_some());
                result
            })
            .join()
            .unwrap()
    })
}

fn claim(
    endpoint: &chigiri::Endpoint<()>,
    ingress: WebSocketUpgradeIngress<'static>,
) -> ClaimedWebSocketUpgrade<'static> {
    match ingress {
        WebSocketUpgradeIngress::Admitted(seed) => {
            endpoint.__claim_websocket_upgrade(seed).unwrap()
        }
        WebSocketUpgradeIngress::Finite(response) => {
            panic!("unexpected rejection {}", response.status)
        }
    }
}

#[test]
fn opening_retention_drops_optional_parent_at_exact_capacity_and_never_keeps_live_scope() {
    let baseline = endpoint(None, 16 * 1024);
    let seed = claim(&baseline, admit(&baseline, false));
    let required = seed.charged_bytes();
    assert!(required > 1024);
    seed.cancel();
    let legacy_seed = claim(&baseline, admit_with_legacy(&baseline, false, true));
    let legacy_capacity = legacy_seed.charged_bytes();
    assert_eq!(
        legacy_seed.request_telemetry().trace_id().unwrap().as_str(),
        "legacy"
    );
    legacy_seed.cancel();
    for suppressed in [false, true] {
        let (_provider, tracer, records) = recording::provider();
        let calls = Arc::clone(&tracer.calls);
        let large = endpoint(Some(tracer.clone()), 16 * 1024);
        let seed = claim(&large, admit(&large, suppressed));
        assert!(seed.request_telemetry().instrumentation_scope().is_none());
        let full = seed.inspect_transport(|request| {
            request
                .__instrumentation_capture()
                .unwrap()
                .values()
                .clone()
        });
        assert_eq!(full.is_suppressed(), suppressed);
        assert_eq!(
            full.parent().unwrap().span_id(),
            parent(8).span_id().to_bytes()
        );
        assert_eq!(full.parent().unwrap().trace_state(), "vendor=value");
        let id_bytes = seed.request_telemetry().trace_id().unwrap().as_str().len();
        let optional = full.correlation_bytes() + id_bytes;
        assert_eq!(seed.charged_bytes(), required + optional);
        seed.cancel();
        for capacity in [required, required + optional - 1, required + optional] {
            let endpoint = endpoint(Some(tracer.clone()), capacity);
            let seed = claim(&endpoint, admit(&endpoint, suppressed));
            let telemetry = seed.request_telemetry();
            assert!(telemetry.instrumentation_scope().is_none());
            assert_eq!(
                telemetry.trace_id().is_some(),
                capacity >= required + id_bytes
            );
            assert_eq!(
                telemetry.propagation_context().unwrap().is_suppressed(),
                suppressed
            );
            let retained = seed.inspect_transport(|request| {
                request
                    .__instrumentation_capture()
                    .unwrap()
                    .values()
                    .clone()
            });
            assert_eq!(retained.is_suppressed(), suppressed);
            assert_eq!(retained.parent().is_some(), capacity == required + optional);
            let trace_bytes = seed
                .request_telemetry()
                .trace_id()
                .map_or(0, |id| id.as_str().len());
            assert_eq!(
                seed.charged_bytes(),
                required + trace_bytes + retained.correlation_bytes()
            );
            seed.cancel();
        }
        let legacy = endpoint(Some(tracer.clone()), legacy_capacity);
        let seed = claim(&legacy, admit_with_legacy(&legacy, suppressed, true));
        assert!(seed.request_telemetry().trace_id().is_none());
        assert_eq!(
            seed.inspect_transport(|request| request.trace_id().unwrap().as_str().to_owned()),
            "legacy"
        );
        let retained = seed.inspect_transport(|request| {
            request
                .__instrumentation_capture()
                .unwrap()
                .values()
                .clone()
        });
        assert!(retained.parent().is_none());
        assert_eq!(retained.is_suppressed(), suppressed);
        seed.cancel();
        let limited = endpoint(Some(tracer), required - 1);
        match admit(&limited, suppressed) {
            WebSocketUpgradeIngress::Finite(response) => assert_eq!(response.status, 413),
            WebSocketUpgradeIngress::Admitted(_) => {
                panic!("required metadata still obeys its limit")
            }
        }
        let ended = records.take();
        assert_eq!(ended.len(), calls.load(Ordering::SeqCst));
        if suppressed {
            assert!(ended.is_empty());
        } else {
            assert_eq!(
                ended
                    .iter()
                    .filter(|span| span.name == "graphql.request")
                    .count(),
                6
            );
        }
    }
}
