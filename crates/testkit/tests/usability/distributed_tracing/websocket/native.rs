//! Both native bridges move opening context into actual authorization workers.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri_opentelemetry::OpenTelemetryInstrumentation;
use futures_util::{SinkExt, StreamExt};
use opentelemetry::trace::{SpanContext, SpanId, TraceState};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

use super::super::{
    parent, query, recording,
    server::{Bridge, Server},
    workers,
};
use super::authorization::{Authorize, Check, Errors};

#[test]
fn native_websocket_opening_adopts_before_handoff_and_authorization_keeps_control_and_disposal() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for suppressed in [false, true] {
                for source in ["current", "header", "absent"] {
                    for fail in [false, true] {
                        let (_provider, tracer, records) = recording::provider();
                        let calls = Arc::clone(&tracer.calls);
                        let selected = match source {
                            "current" => parent(7),
                            "header" => {
                                let span = parent(8);
                                SpanContext::new(
                                    span.trace_id(),
                                    span.span_id(),
                                    span.trace_flags(),
                                    true,
                                    TraceState::from_key_value([("vendor", "value")]).unwrap(),
                                )
                            }
                            _ => SpanContext::NONE,
                        };
                        let check = Arc::new(Check {
                            selected: selected.clone(),
                            suppressed,
                            stages: Default::default(),
                        });
                        let endpoint = chigiri::Endpoint::builder(query::resolvers())
                            .inject(|components| {
                                components
                                    .observability()
                                    .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                                    .error_observer(Errors(Arc::clone(&check)));
                            })
                            .connection_init_authorizer(Authorize {
                                check: Arc::clone(&check),
                                fail,
                            })
                            .without_context()
                            .build()
                            .unwrap();
                        let server = Server::start(
                            endpoint.clone(),
                            bridge,
                            source == "current",
                            suppressed,
                        )
                        .await;
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
                        if source != "absent" {
                            request.headers_mut().insert(
                                "traceparent",
                                "00-08080808080808080808080808080808-0808080808080808-01"
                                    .parse()
                                    .unwrap(),
                            );
                            request
                                .headers_mut()
                                .insert("tracestate", "vendor=value".parse().unwrap());
                        }
                        let (mut socket, response) = tokio::time::timeout(
                            Duration::from_secs(2),
                            tokio_tungstenite::connect_async(request),
                        )
                        .await
                        .unwrap()
                        .unwrap();
                        assert_eq!(response.status(), 101);
                        assert!(check.stages.lock().unwrap().is_empty());
                        // Opening request completion precedes any connection-init work.
                        let ended = records.take();
                        assert_eq!(ended.len(), calls.load(Ordering::SeqCst));
                        let roots = ended
                            .iter()
                            .filter(|span| span.name == "graphql.request")
                            .collect::<Vec<_>>();
                        assert_eq!(roots.len(), usize::from(!suppressed));
                        if !suppressed {
                            assert_eq!(
                                roots[0].parent_span_id,
                                if selected.is_valid() {
                                    selected.span_id()
                                } else {
                                    SpanId::INVALID
                                }
                            );
                        }
                        tokio::time::timeout(
                            Duration::from_secs(2),
                            socket.send(Message::Text("{\"type\":\"connection_init\"}".into())),
                        )
                        .await
                        .unwrap()
                        .unwrap();
                        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
                            .await
                            .unwrap()
                            .unwrap()
                            .unwrap();
                        if fail {
                            let Message::Close(Some(close)) = message else {
                                panic!("safe authorization close")
                            };
                            assert_eq!(u16::from(close.code), 1011);
                            assert!(!close.reason.contains("private"));
                        } else {
                            let Message::Text(text) = message else {
                                panic!("connection acknowledgment")
                            };
                            assert_eq!(
                                serde_json::from_str::<serde_json::Value>(&text).unwrap()["type"],
                                "connection_ack"
                            );
                            tokio::time::timeout(Duration::from_secs(2), socket.close(None))
                                .await
                                .unwrap()
                                .unwrap();
                        }
                        drop(socket);
                        server.stop().await;
                        assert_eq!(
                            endpoint.shutdown_outcome(),
                            Some(chigiri::ShutdownOutcome::Drained)
                        );
                        let mut expected = vec!["construct", "poll", "resume", "future-drop"];
                        if fail {
                            expected.extend(["error-observer", "source-drop"]);
                        }
                        assert_eq!(
                            *check.stages.lock().unwrap(),
                            expected,
                            "{bridge:?}/{suppressed}/{source}/{fail}"
                        );
                        assert!(records.take().is_empty());
                        assert_eq!(ended.len(), calls.load(Ordering::SeqCst));
                    }
                }
            }
        }
    });
}
