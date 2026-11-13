//! Cross-setting failures retain deterministic classification and graph origins.

use chigiri::{
    ConfigurationConstraint as Constraint, ConfigurationOrigin as Origin,
    ConfigurationSlot as Slot, ConfigurationValue as Value, ConfigurationViolation as Violation,
    Endpoint, SubscriptionLimits as Limits, SubscriptionProtocolSettings as Protocol,
};

use super::{build, diagnostic, resolvers};

#[test]
fn both_fit_conflicts_are_explicit_and_queue_wins_if_both_fail() {
    for (queue, endpoint, expected) in [
        (262_144, 67_108_864, Slot::QueuedSubscriptionBytes),
        (2_097_152, 262_144, Slot::EndpointSubscriptionEncodedBytes),
        (262_144, 262_144, Slot::QueuedSubscriptionBytes),
    ] {
        let limits = Limits {
            max_encoded_event_bytes: 786_432,
            max_queued_bytes: queue,
            max_endpoint_encoded_bytes: endpoint,
            ..Limits::default()
        };
        let sugar = diagnostic(build(limits, Protocol::default(), false));
        let injected = diagnostic(build(limits, Protocol::default(), true));
        assert_eq!(sugar, injected);
        assert_eq!(sugar.slot(), Slot::EncodedSubscriptionEventBytes);
        assert_eq!(sugar.origin(), Origin::User);
        assert_eq!(sugar.violation(), Violation::InconsistentLimits);
        let Some(Constraint::Fit { supplied, capacity }) = sugar.constraint() else {
            panic!("expected a fit constraint");
        };
        assert_eq!(supplied, 786_432);
        assert_eq!(capacity.slot(), expected);
        assert_eq!(capacity.value(), Value::Bytes(262_144));
        assert_eq!(capacity.origin(), Origin::User);
        assert_eq!(format!("{sugar:?}"), sugar.to_string());
        assert!(sugar.to_string().contains("786432 bytes"));
        assert!(sugar.to_string().contains("262144 bytes"));
    }
}

#[test]
fn overflow_reports_only_operands_and_precedes_invalid_operand_ranges() {
    let limits = Limits {
        max_queued_events: usize::MAX,
        max_encoded_event_bytes: 2,
        max_active_subscriptions: 0,
        ..Limits::default()
    };
    let sugar = diagnostic(build(limits, Protocol::default(), false));
    assert_eq!(sugar, diagnostic(build(limits, Protocol::default(), true)));
    assert_eq!(sugar.slot(), Slot::QueuedSubscriptionBytes);
    assert_eq!(sugar.violation(), Violation::ArithmeticOverflow);
    let Some(Constraint::MultiplicationOverflow { left, right }) = sugar.constraint() else {
        panic!("expected checked multiplication operands");
    };
    assert_eq!(left.slot(), Slot::QueuedSubscriptionEvents);
    assert_eq!(left.value(), Value::Count(usize::MAX));
    assert_eq!(left.origin(), Origin::User);
    assert_eq!(right.slot(), Slot::EncodedSubscriptionEventBytes);
    assert_eq!(right.value(), Value::Bytes(2));
    assert_eq!(right.origin(), Origin::User);
    assert_eq!(format!("{sugar:?}"), sugar.to_string());
    assert!(!sugar.to_string().contains("product"));
}

#[test]
fn graph_overrides_can_repair_temporary_inconsistency_without_enlarging_budgets() {
    let invalid = Limits {
        max_encoded_event_bytes: 786_432,
        ..Limits::default()
    };
    let valid = Limits {
        max_queued_bytes: 2_097_152,
        ..invalid
    };
    let rejected = diagnostic(
        Endpoint::builder(resolvers())
            .subscription_limits(valid)
            .inject(|c| {
                c.subscriptions().limits(invalid);
            })
            .without_context()
            .build(),
    );
    assert_eq!(
        rejected,
        diagnostic(build(invalid, Protocol::default(), false))
    );
    let repaired = Endpoint::builder(resolvers())
        .subscription_limits(invalid)
        .inject(|c| {
            c.subscriptions().limits(valid);
        })
        .without_context()
        .build()
        .unwrap();
    let direct = build(valid, Protocol::default(), false).unwrap();
    assert_eq!(
        repaired.configuration().subscription_limits(),
        direct.configuration().subscription_limits()
    );
    assert_eq!(
        *repaired.configuration().subscription_limits().effective(),
        valid
    );
    assert_eq!(
        valid.max_endpoint_encoded_bytes,
        Limits::default().max_endpoint_encoded_bytes
    );
    assert_eq!(
        repaired.configuration().subscription_protocol().origin(),
        Origin::Baseline
    );
}

#[test]
fn queue_count_is_independent_from_byte_capacity_and_both_fits_are_inclusive() {
    let limits = Limits {
        max_encoded_event_bytes: 1_048_576,
        max_queued_bytes: 1_048_576,
        max_endpoint_encoded_bytes: 1_048_576,
        max_queued_events: 1_024,
        ..Limits::default()
    };
    let endpoint = build(limits, Protocol::default(), false).unwrap();
    assert_eq!(
        *endpoint.configuration().subscription_limits().effective(),
        limits
    );
}

#[test]
fn numeric_range_debug_display_and_error_use_exact_byte_numbers() {
    let error = build(
        Limits {
            max_encoded_event_bytes: 1_048_577,
            ..Limits::default()
        },
        Protocol::default(),
        false,
    )
    .err()
    .unwrap();
    for output in [format!("{error:?}"), error.to_string()] {
        for expected in [
            "EncodedSubscriptionEventBytes",
            "User",
            "OutsideHardBound",
            "supplied: 1048577 bytes",
            "minimum: 1 bytes",
            "maximum: 1048576 bytes",
        ] {
            assert!(output.contains(expected), "{output}");
        }
    }
}
