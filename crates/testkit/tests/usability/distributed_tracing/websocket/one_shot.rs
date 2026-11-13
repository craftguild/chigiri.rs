//! Finite operations use opening context throughout real native worker handoffs.

use std::future::poll_fn;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use futures_util::{SinkExt, StreamExt};
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use super::super::{
    PrivateValue, parent, query, recording, scopes,
    server::{Bridge, Server},
    workers,
};

type Seen = Arc<Mutex<Vec<(&'static str, SpanContext, bool, bool)>>>;

fn observe(seen: &Seen, stage: &'static str) {
    let context = Context::current();
    seen.lock().unwrap().push((
        stage,
        context.span().span_context().clone(),
        context.is_telemetry_suppressed(),
        context.get::<PrivateValue>().is_some(),
    ));
}

struct Factory(Seen);
struct Disposal(Seen);
impl Drop for Disposal {
    fn drop(&mut self) {
        observe(&self.0, "future-drop");
    }
}

impl chigiri_http::ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        _: chigiri_http::RequestContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::PreparedContext<(), ()>, chigiri::PrepareError>>
    {
        observe(&self.0, "construct");
        let disposal = Disposal(Arc::clone(&self.0));
        Box::pin(async move {
            observe(&self.0, "poll");
            let mut pending = true;
            poll_fn(|context| {
                if pending {
                    pending = false;
                    context.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            observe(&self.0, "resume");
            drop(disposal);
            Ok(chigiri::PreparedContext::execution_only(()))
        })
    }
}

struct Sampling {
    calls: Arc<AtomicUsize>,
    decision: chigiri::SamplingDecision,
}
impl chigiri::Sampler for Sampling {
    fn sample(&self, _: chigiri::RequestIdentity<'_>) -> chigiri::SamplingDecision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.decision
    }
}

#[test]
fn native_one_shot_keeps_opening_parent_control_and_fixed_sampling() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for suppressed in [false, true] {
                for has_parent in [false, true] {
                    for decision in [
                        chigiri::SamplingDecision::Record,
                        chigiri::SamplingDecision::Drop,
                    ] {
                        for mutation in [false, true] {
                            run_case(bridge, suppressed, has_parent, decision, mutation).await;
                        }
                    }
                }
            }
        }
    });
}

async fn run_case(
    bridge: Bridge,
    suppressed: bool,
    has_parent: bool,
    decision: chigiri::SamplingDecision,
    mutation: bool,
) {
    let (_provider, tracer, records) = recording::provider();
    let span_calls = Arc::clone(&tracer.calls);
    let samples = Arc::new(AtomicUsize::new(0));
    let seen = Seen::default();
    let endpoint = chigiri::Endpoint::builder(query::one_shot_resolvers())
        .inject(|components| {
            components
                .observability()
                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                .sampler(Sampling {
                    calls: Arc::clone(&samples),
                    decision,
                });
        })
        .context_factory(Factory(Arc::clone(&seen)))
        .build()
        .unwrap();
    let server = Server::start(endpoint.clone(), bridge, has_parent, suppressed).await;
    let mut request = format!("ws://{}/graphql", server.address)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "sec-websocket-protocol",
        "graphql-transport-ws".parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("x-chigiri-preflight", "1".parse().unwrap());
    if has_parent {
        // The current middleware parent wins over conflicting headers.
        request.headers_mut().insert(
            "traceparent",
            "00-08080808080808080808080808080808-0808080808080808-01"
                .parse()
                .unwrap(),
        );
    }
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .unwrap()
    .unwrap();
    socket
        .send(Message::Text("{\"type\":\"connection_init\"}".into()))
        .await
        .unwrap();
    let ack = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(ack.to_text().unwrap().contains("connection_ack"));
    let opening = records.take();
    assert_eq!(opening.len(), span_calls.load(Ordering::SeqCst));
    let before = samples.load(Ordering::SeqCst);
    let (document, field) = if mutation {
        ("mutation { rename(name: \"ignored\") }", "rename")
    } else {
        ("{ greeting }", "greeting")
    };
    let message =
        serde_json::json!({"id": "one", "type": "subscribe", "payload": {"query": document}});
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
    assert_eq!(response["type"], "next", "{response}");
    assert!(response["payload"].get("errors").is_none(), "{response}");
    let complete = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(complete.to_text().unwrap().contains("complete"));
    socket.close(None).await.unwrap();
    drop(socket);
    server.stop().await;
    assert_eq!(
        endpoint.shutdown_outcome(),
        Some(chigiri::ShutdownOutcome::Drained)
    );
    assert_eq!(
        samples.load(Ordering::SeqCst) - before,
        1,
        "one fixed operation decision"
    );
    let ended = records.take();
    assert_eq!(
        opening.len() + ended.len(),
        span_calls.load(Ordering::SeqCst)
    );
    let recording = !suppressed && decision == chigiri::SamplingDecision::Record;
    let roots = ended
        .iter()
        .filter(|span| span.name == "graphql.request")
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), usize::from(recording));
    let selected = if has_parent {
        parent(7)
    } else {
        SpanContext::NONE
    };
    let (factory, resolver) = if recording {
        let root = roots[0];
        assert_eq!(
            root.parent_span_id,
            if has_parent {
                selected.span_id()
            } else {
                SpanId::INVALID
            }
        );
        if has_parent {
            assert_eq!(root.span_context.trace_id(), selected.trace_id());
        }
        (
            scopes::context_factory(&ended, &root.span_context),
            scopes::execution(&ended, &root.span_context),
        )
    } else {
        assert!(ended.is_empty());
        (selected.clone(), selected)
    };
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    for ((stage, actual, control, private), expected_stage) in
        seen.iter()
            .zip(["construct", "poll", "resume", "future-drop"])
    {
        assert_eq!(*stage, expected_stage);
        assert_eq!(
            *actual, factory,
            "{bridge:?}/{suppressed}/{has_parent}/{decision:?}/{stage}"
        );
        assert_eq!(*control, suppressed);
        assert!(!private);
    }
    assert_eq!(
        response["payload"]["data"][field],
        format!(
            "{}:{}:{}",
            resolver.trace_id(),
            resolver.span_id(),
            suppressed
        )
    );
}
