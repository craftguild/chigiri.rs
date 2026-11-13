use chigiri_runtime::SamplingDecision;
use chigiri_runtime::instrumentation::{Instrumentation, InstrumentationKind, PropagationField};
use opentelemetry::Context;
use opentelemetry::trace::{TraceContextExt, TraceState};

use super::support::{Headers, MustNotRead, PARENT, parent, provider};

#[derive(Clone)]
struct PrivateValue;

#[test]
fn capture_selects_owning_parent_and_control_before_fallback_without_private_values() {
    let (_owner, backend, _) = provider();
    for suppressed in [false, true] {
        for valid_parent in [false, true] {
            let base = if valid_parent {
                parent(7, true)
            } else {
                Context::new()
            };
            let base = base.with_value(PrivateValue);
            let base = if suppressed {
                base.with_telemetry_suppressed()
            } else {
                base
            };
            let _guard = base.attach();
            let values = if valid_parent {
                backend.capture(&MustNotRead)
            } else {
                backend.capture(&Headers::parent())
            };
            assert_eq!(values.is_suppressed(), suppressed);
            let selected = values.parent().unwrap();
            assert_eq!(
                selected.trace_id(),
                if valid_parent {
                    [7; 16]
                } else {
                    [
                        1, 35, 69, 103, 137, 171, 205, 239, 1, 35, 69, 103, 137, 171, 205, 239,
                    ]
                }
            );
            let scope = backend.start(
                &values,
                InstrumentationKind::Request,
                SamplingDecision::Drop,
            );
            scope.run(|| {
                assert_eq!(Context::current().is_telemetry_suppressed(), suppressed);
                assert!(Context::current().get::<PrivateValue>().is_none());
                assert_eq!(
                    Context::current()
                        .span()
                        .span_context()
                        .trace_id()
                        .to_bytes(),
                    selected.trace_id()
                );
            });
            assert!(Context::current().get::<PrivateValue>().is_some());
        }
    }
}

#[test]
fn bounded_released_extraction_rejects_parent_faults_and_degrades_only_bad_state() {
    let (_owner, backend, _) = provider();
    let _guard = Context::new().with_telemetry_suppressed().attach();
    let parent_field = PropagationField::TraceParent;
    for headers in [
        Headers(vec![(parent_field, PARENT.as_bytes().to_vec()); 2]),
        Headers(vec![(parent_field, vec![b'a'; 513])]),
        Headers(vec![(parent_field, b"malformed".to_vec())]),
        Headers(vec![(
            parent_field,
            PARENT.replace("00-", "ff-").into_bytes(),
        )]),
        Headers(vec![(parent_field, PARENT.to_uppercase().into_bytes())]),
        Headers(vec![(parent_field, vec![255; 55])]),
        Headers(vec![(parent_field, format!("{PARENT}\n").into_bytes())]),
    ] {
        let values = backend.capture(&headers);
        assert!(values.parent().is_none());
        assert!(values.is_suppressed());
    }
    let states = [
        "vendor=".to_owned() + &"x".repeat(506),
        "invalid=has=equals".to_owned(),
        "vendor=control\u{0}".to_owned(),
        (0..33)
            .map(|n| format!("a{n}=v"))
            .collect::<Vec<_>>()
            .join(","),
    ];
    for state in states {
        let mut headers = Headers::parent();
        headers
            .0
            .push((PropagationField::TraceState, state.into_bytes()));
        let values = backend.capture(&headers);
        assert_eq!(values.parent().unwrap().trace_state(), "");
        assert!(values.is_suppressed());
    }
    let mut headers = Headers::parent();
    headers
        .0
        .push((PropagationField::TraceState, b" first=value ".to_vec()));
    headers
        .0
        .push((PropagationField::TraceState, b"second=other".to_vec()));
    assert_eq!(
        backend.capture(&headers).parent().unwrap().trace_state(),
        "first=value,second=other"
    );

    let exact = (0..32)
        .map(|n| format!("a{n}=v"))
        .collect::<Vec<_>>()
        .join(",");
    headers.0.truncate(1);
    headers
        .0
        .push((PropagationField::TraceState, exact.clone().into_bytes()));
    assert_eq!(
        backend.capture(&headers).parent().unwrap().trace_state(),
        exact
    );
    let exact_bytes = format!("a={},b={}", "x".repeat(254), "y".repeat(253));
    assert_eq!(exact_bytes.len(), 512);
    headers.0.truncate(1);
    headers.0.push((
        PropagationField::TraceState,
        exact_bytes.clone().into_bytes(),
    ));
    assert_eq!(
        backend.capture(&headers).parent().unwrap().trace_state(),
        exact_bytes
    );
    headers.0.truncate(1);
    headers.0.push((
        PropagationField::TraceState,
        [exact_bytes.as_bytes(), b" "].concat(),
    ));
    assert_eq!(
        backend.capture(&headers).parent().unwrap().trace_state(),
        ""
    );

    let mut future_parent = PARENT.replacen("00-", "01-", 1);
    future_parent.push('-');
    future_parent.push_str(&"x".repeat(456));
    assert_eq!(future_parent.len(), 512);
    let exact_parent = Headers(vec![(parent_field, future_parent.clone().into_bytes())]);
    assert!(backend.capture(&exact_parent).parent().is_some());
    future_parent.push('x');
    assert!(
        backend
            .capture(&Headers(vec![(parent_field, future_parent.into_bytes())]))
            .parent()
            .is_none()
    );
    headers.0.truncate(1);
    headers
        .0
        .push((PropagationField::TraceState, vec![b' '; 512]));
    headers.0.push((PropagationField::TraceState, Vec::new()));
    assert_eq!(
        backend.capture(&headers).parent().unwrap().trace_state(),
        ""
    );
}

#[test]
fn oversized_ambient_trace_state_is_bounded_before_rendering() {
    let (_owner, backend, _) = provider();
    let state = TraceState::from_key_value((0..33).map(|n| (format!("v{n}"), "value"))).unwrap();
    let original = parent(3, true);
    let identity = original.span().span_context().clone();
    let identity = opentelemetry::trace::SpanContext::new(
        identity.trace_id(),
        identity.span_id(),
        identity.trace_flags(),
        false,
        state,
    );
    let _guard = Context::new().with_remote_span_context(identity).attach();
    let selected = backend.capture(&MustNotRead);
    assert_eq!(selected.parent().unwrap().trace_id(), [3; 16]);
    assert_eq!(selected.parent().unwrap().trace_state(), "");
}
