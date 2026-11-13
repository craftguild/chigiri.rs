//! Actual HTTP on both SDK-managed bridges with unrelated worker ambient state.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_opentelemetry::OpenTelemetryInstrumentation;

use super::server::{Bridge, Server};
use super::{FirstIdentityOnly, Observed, client, http, parent, query, recording, workers};

#[test]
fn managed_axum_and_hyper_adopt_before_tasks_and_preserve_suppression() {
    workers::runtime().block_on(async {
        for bridge in [Bridge::Axum, Bridge::Hyper] {
            for suppressed in [false, true] {
                for current in [false, true] {
                    let (_provider, tracer, records) = recording::provider();
                    let calls = Arc::clone(&tracer.calls);
                    let observed = Observed::default();
                    let seen = http::Seen::default();
                    let endpoint = chigiri::Endpoint::builder(query::resolvers())
                        .inject(|components| {
                            components.observability()
                                .instrumentation(OpenTelemetryInstrumentation::new(tracer))
                                .request_id_factory(FirstIdentityOnly(AtomicUsize::new(0)))
                                .lifecycle_observer(observed.clone())
                                .error_observer(observed.clone());
                        })
                        .context_factory(http::Factory(Arc::clone(&seen))).build().unwrap();
                    let server = Server::start(endpoint.clone(), bridge, current, suppressed).await;
                    let headers = "traceparent: 00-08080808080808080808080808080808-0808080808080808-01\r\ntracestate: vendor=value\r\n";
                    let mut responses = Vec::new();
                    for index in 0..2 {
                        let response = if current && index == 1 {
                            client::request(server.address, "/graphql", Some("{ greeting }")).await
                        } else {
                            client::request_with_headers(server.address, "/graphql", Some("{ greeting }"), headers).await
                        };
                        responses.push(response.unwrap());
                    }
                    server.stop().await;
                    assert_eq!(endpoint.shutdown_outcome(), Some(chigiri::ShutdownOutcome::Drained));
                    let ended = records.take();
                    let roots = ended.iter().filter(|span| span.name == "graphql.request").collect::<Vec<_>>();
                    assert_eq!(roots.len(), if suppressed { 0 } else { 2 }, "{bridge:?}");
                    assert_eq!(ended.len(), calls.load(Ordering::SeqCst));
                    if suppressed { assert_eq!(calls.load(Ordering::SeqCst), 0); }
                    let expected = parent(if current { 7 } else { 8 });
                    let seen = seen.lock().unwrap();
                    assert_eq!(seen.len(), 6);
                    for (index, response) in responses.into_iter().enumerate() {
                        assert_eq!(response.status, hyper::StatusCode::OK);
                        let span = &seen[index*3].0;
                        assert_eq!(span.trace_id(), expected.trace_id());
                        assert!(seen[index*3..index*3+3].iter().all(|(value, control)| value == span && *control == suppressed));
                        if !suppressed {
                            assert_eq!(roots[index].parent_span_id, expected.span_id());
                            assert_eq!(super::scopes::context_factory(&ended, &roots[index].span_context), *span);
                        }
                        let resolver = if suppressed { span.clone() } else { super::scopes::execution(&ended, &roots[index].span_context) };
                        let json: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
                        assert_eq!(json["data"]["greeting"], format!("{}:{}:{}", resolver.trace_id(), resolver.span_id(), suppressed));
                    }
                    assert_eq!(observed.errors.load(Ordering::SeqCst), 1);
                }
            }
        }
    });
}
