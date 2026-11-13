//! Assertions use actual SDK IDs, creation-time links, public data and callback probes.

use chigiri_http::{RequestPhase, SubscriptionTransport};
use opentelemetry::trace::{SpanContext, SpanId};
use opentelemetry_sdk::trace::SpanData;
use serde_json::Value;

use super::super::{parent, scopes};
use super::fixture::Probe;

pub(super) fn check(
    spans: &[SpanData],
    probe: &Probe,
    data: &[Value],
    suppressed: bool,
    has_parent: bool,
    setup: chigiri::SamplingDecision,
    event: chigiri::SamplingDecision,
) {
    let setup_recorded = !suppressed && setup == chigiri::SamplingDecision::Record;
    let event_recorded = !suppressed && event == chigiri::SamplingDecision::Record;
    let requests = spans
        .iter()
        .filter(|span| span.name == "graphql.request")
        .collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        usize::from(setup_recorded)
            * (1 + usize::from(probe.profile == SubscriptionTransport::WebSocket))
    );
    let incoming = if has_parent {
        parent(7)
    } else {
        SpanContext::NONE
    };
    let setup_context = if setup_recorded {
        let setup = requests.last().unwrap();
        assert_eq!(
            setup.parent_span_id,
            if has_parent {
                incoming.span_id()
            } else {
                SpanId::INVALID
            }
        );
        setup.span_context.clone()
    } else {
        incoming
    };
    let events = spans
        .iter()
        .filter(|span| span.name == "graphql.subscription.event")
        .collect::<Vec<_>>();
    assert_eq!(events.len(), usize::from(event_recorded) * 2);
    if event_recorded {
        assert_ne!(
            events[0].span_context.trace_id(),
            events[1].span_context.trace_id()
        );
        for event in &events {
            assert_eq!(event.parent_span_id, SpanId::INVALID);
            assert_ne!(event.span_context.trace_id(), setup_context.trace_id());
            assert_eq!(
                event.links.links.len(),
                usize::from(setup_context.is_valid())
            );
            if setup_context.is_valid() {
                assert_eq!(event.links.links[0].span_context, setup_context);
            }
            if setup_recorded {
                assert!(requests.last().unwrap().end_time <= event.start_time);
            }
        }
    }
    let seen = probe.seen.lock().unwrap();
    assert_eq!(
        seen.iter().filter(|seen| seen.stage == "subscribe").count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|seen| seen.stage == "source-drop")
            .count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|seen| seen.stage == "source-poll")
            .count(),
        4
    );
    for seen in seen.iter() {
        assert_eq!(seen.suppressed, suppressed, "{}", seen.stage);
        assert!(!seen.private, "{}", seen.stage);
        if seen.stage.starts_with("source-") {
            assert_eq!(seen.span, setup_context, "{}", seen.stage);
        }
    }
    assert_eq!(data.len(), 2);
    for sequence in 1..=2 {
        let root = if event_recorded {
            events[sequence - 1].span_context.clone()
        } else {
            SpanContext::NONE
        };
        let factory = if event_recorded {
            scopes::context_factory(spans, &root)
        } else {
            SpanContext::NONE
        };
        let execution = if event_recorded {
            let executions = spans
                .iter()
                .filter(|span| {
                    span.name == "graphql.operation.execute"
                        && span.parent_span_id == root.span_id()
                })
                .collect::<Vec<_>>();
            assert_eq!(executions.len(), 1);
            executions[0].span_context.clone()
        } else {
            SpanContext::NONE
        };
        let phase = RequestPhase::SubscriptionEvent {
            transport: probe.profile,
            sequence: sequence as u64,
        };
        let entries = seen
            .iter()
            .filter(|seen| seen.phase == phase)
            .collect::<Vec<_>>();
        assert_eq!(
            entries
                .iter()
                .filter(|seen| seen.stage.starts_with("factory-"))
                .count(),
            4
        );
        assert_eq!(
            entries
                .iter()
                .filter(|seen| seen.stage == "state-drop")
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|seen| seen.stage == "event-drop")
                .count(),
            2
        );
        for entry in entries {
            if entry.stage.starts_with("factory-") {
                assert_eq!(entry.span, factory, "{}", entry.stage);
            }
            if matches!(
                entry.stage,
                "factory-construct" | "factory-poll" | "factory-resume"
            ) {
                assert_eq!(
                    entry.trace_id,
                    root.is_valid().then(|| root.trace_id().to_string())
                );
            }
            if entry.stage == "state-drop" {
                assert_eq!(entry.span, root);
            }
            if entry.stage.starts_with("resolve") || entry.stage.starts_with("padding") {
                assert_eq!(entry.span, execution, "{}", entry.stage);
            }
            if entry.stage == "event-drop" {
                assert_eq!(entry.span.trace_id(), root.trace_id());
            }
        }
        let value = &data[sequence - 1];
        assert!(value.get("errors").is_none(), "{value}");
        assert_eq!(value["data"]["ticks"]["value"], sequence);
        assert_eq!(
            value["data"]["ticks"]["padding"],
            format!(
                "{}:{}:{}",
                execution.trace_id(),
                execution.span_id(),
                suppressed
            )
        );
        assert_eq!(
            value.pointer("/extensions/chigiri/delivery/droppedTotal"),
            Some(&Value::Null)
        );
    }
    if suppressed {
        assert!(spans.is_empty());
    }
}
