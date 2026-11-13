//! Local assertions use typed outputs and measured standard SDK contexts.

use chigiri::__streaming::SubscriptionOutput;
use opentelemetry::trace::{SpanContext, SpanId};
use opentelemetry_sdk::trace::SpanData;

use super::super::super::{parent, scopes};
use super::{End, fixture};

pub(super) fn output(output: SubscriptionOutput, end: End) {
    let response = match output {
        SubscriptionOutput::Event(response) if matches!(end, End::Success) => response,
        SubscriptionOutput::Terminal(response) if !matches!(end, End::Success) => {
            response.into_parts().0
        }
        _ => panic!("unexpected typed event output for {end:?}"),
    };
    match end {
        End::Success => assert!(response.errors.is_empty()),
        End::Reject => assert_eq!(response.errors[0].message, "event denied"),
        End::Internal | End::TimeoutFactory | End::TimeoutResolver => {
            let code = if matches!(end, End::Internal) {
                "INTERNAL_SERVER_ERROR"
            } else {
                "REQUEST_TIMEOUT"
            };
            assert_eq!(response.errors.len(), 1);
            assert_eq!(
                response.errors[0].extensions.get("code"),
                Some(&chigiri::Value::String(code.into()))
            );
            assert!(!format!("{response:?}").contains("private event failure"));
        }
        _ => unreachable!(),
    }
}

pub(super) fn contexts(
    spans: &[SpanData],
    probe: &fixture::Probe,
    suppressed: bool,
    has_parent: bool,
    decision: chigiri::SamplingDecision,
    end: End,
) {
    let recorded = !suppressed && decision == chigiri::SamplingDecision::Record;
    let setups = spans
        .iter()
        .filter(|span| span.name == "graphql.request")
        .collect::<Vec<_>>();
    let events = spans
        .iter()
        .filter(|span| span.name == "graphql.subscription.event")
        .collect::<Vec<_>>();
    assert_eq!(setups.len(), usize::from(recorded));
    assert_eq!(events.len(), usize::from(recorded));
    let setup = if recorded {
        setups[0].span_context.clone()
    } else if has_parent {
        parent(7)
    } else {
        SpanContext::NONE
    };
    let root = if recorded {
        assert_eq!(events[0].parent_span_id, SpanId::INVALID);
        assert_ne!(events[0].span_context.trace_id(), setup.trace_id());
        assert_eq!(events[0].links.links.len(), 1);
        assert_eq!(events[0].links.links[0].span_context, setup);
        assert!(setups[0].end_time <= events[0].start_time);
        events[0].span_context.clone()
    } else {
        SpanContext::NONE
    };
    let factory = if recorded {
        scopes::context_factory(spans, &root)
    } else {
        root.clone()
    };
    let executes = matches!(
        end,
        End::Success | End::CancelResolver | End::TimeoutResolver | End::PanicResolver
    );
    let executions = spans
        .iter()
        .filter(|span| span.name == "graphql.operation.execute")
        .collect::<Vec<_>>();
    assert_eq!(executions.len(), usize::from(recorded && executes));
    let execution = if recorded && executes {
        assert_eq!(executions[0].parent_span_id, root.span_id());
        executions[0].span_context.clone()
    } else {
        root.clone()
    };
    let seen = probe.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|seen| seen.stage == "source-drop")
            .count(),
        1
    );
    let entries = seen
        .iter()
        .filter(|seen| seen.phase == probe.event(1))
        .collect::<Vec<_>>();
    assert_eq!(
        entries
            .iter()
            .filter(|seen| seen.stage == "factory-drop")
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|seen| seen.stage == "event-drop")
            .count(),
        1 + usize::from(matches!(end, End::Success))
    );
    assert_eq!(
        entries
            .iter()
            .filter(|seen| seen.stage == "state-drop")
            .count(),
        usize::from(executes)
    );
    assert_eq!(
        entries
            .iter()
            .filter(|seen| seen.stage == "resolver-drop")
            .count(),
        usize::from(executes)
    );
    for seen in seen.iter() {
        assert_eq!(seen.suppressed, suppressed, "{end:?}/{}", seen.stage);
        assert!(!seen.private);
        if seen.stage.starts_with("source-") {
            assert_eq!(seen.span, setup);
        }
        if seen.phase != probe.event(1) {
            continue;
        }
        if seen.stage.starts_with("factory-") {
            assert_eq!(seen.span, factory, "{}", seen.stage);
        }
        if seen.stage == "state-drop" {
            assert_eq!(seen.span, root);
        }
        if seen.stage.starts_with("resolve") || seen.stage.starts_with("padding") {
            assert_eq!(seen.span, execution, "{}", seen.stage);
        }
        if seen.stage == "event-drop" {
            assert_eq!(seen.span.trace_id(), root.trace_id());
        }
    }
    if suppressed {
        assert!(spans.is_empty());
    }
}
