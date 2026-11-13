//! Retention bounds use explicit values, independently of any review record.

use crate::instrumentation::{PropagationContext, TraceParent};

#[test]
fn optional_correlation_capacity_never_discards_standard_suppression() {
    for suppressed in [false, true] {
        let parent = TraceParent::new([1; 16], [2; 8], 1, true, "vendor=private-state").unwrap();
        let context = PropagationContext::new(Some(parent), suppressed);
        let charge = context.correlation_bytes();
        assert!(charge > 0);
        assert_eq!(context.retain_within(charge), context);
        let reduced = context.retain_within(charge - 1);
        assert!(reduced.parent().is_none());
        assert_eq!(reduced.is_suppressed(), suppressed);
        assert_eq!(reduced.correlation_bytes(), 0);
        assert!(!format!("{context:?}").contains("private-state"));
        assert_eq!(reduced.retain_within(0), reduced);
    }
}

#[test]
fn state_limits_preserve_valid_parent_without_copying_oversized_state() {
    let exact = format!("a={},cc={}", "b".repeat(256), "d".repeat(250));
    assert_eq!(exact.len(), 512);
    let parent = TraceParent::new([1; 16], [2; 8], 0, false, &exact).unwrap();
    assert_eq!(parent.trace_state(), exact);
    assert_eq!(parent.trace_id(), [1; 16]);
    assert_eq!(parent.span_id(), [2; 8]);
    assert_eq!(parent.flags(), 0);
    assert!(!parent.is_remote());
    let over_members = (0..33)
        .map(|index| format!("k{index}=v"))
        .collect::<Vec<_>>()
        .join(",");
    for state in [format!("{exact}e"), over_members] {
        let parent = TraceParent::new([1; 16], [2; 8], 1, true, &state).unwrap();
        assert_eq!(parent.trace_state(), "");
        assert_eq!(parent.trace_id(), [1; 16]);
        assert_eq!(parent.span_id(), [2; 8]);
        assert!(parent.is_remote());
    }
    assert!(TraceParent::new([0; 16], [2; 8], 1, false, "").is_none());
    assert!(TraceParent::new([1; 16], [0; 8], 1, false, "").is_none());
}
