use serde_json::json;

use super::super::{EventSettings, EventSettingsError, SourceLimits};

#[test]
fn ev_09_omission_and_partial_limits_have_the_normative_defaults() {
    let settings: EventSettings = serde_json::from_str("{}").unwrap();
    assert!(settings.consumers().is_empty());
    assert_eq!(settings.limits(), SourceLimits::default());
    assert_eq!(
        serde_json::to_value(settings).unwrap(),
        json!({
            "limits": {
                "max_queued_events": 32,
                "max_queued_bytes": 262_144,
                "max_queue_age_ms": 10_000,
                "max_retained_bytes": 16_777_216,
                "max_sequence_bytes": 1_048_576,
            },
            "consumers": {},
        })
    );
    let settings: EventSettings =
        serde_json::from_value(json!({"limits": {"max_queued_events": 1}})).unwrap();
    let limits = settings.limits();
    assert_eq!(limits.max_queued_events(), 1);
    assert_eq!(limits.max_queued_bytes(), 262_144);
    assert_eq!(limits.max_queue_age_ms(), 10_000);
    assert_eq!(limits.max_retained_bytes(), 16_777_216);
    assert_eq!(limits.max_sequence_bytes(), 1_048_576);
}

#[test]
fn ev_09_limits_accept_inclusive_minimum_and_hard_maximum() {
    let minimum = SourceLimits::new(1, 1, 1, 1, 1).unwrap();
    assert_eq!(minimum.max_queued_events(), 1);
    let maximum = json!({
        "max_queued_events": 1_024,
        "max_queued_bytes": 4_194_304,
        "max_queue_age_ms": 60_000,
        "max_retained_bytes": 67_108_864,
        "max_sequence_bytes": 16_777_216,
    });
    let limits: SourceLimits = serde_json::from_value(maximum.clone()).unwrap();
    assert_eq!(serde_json::to_value(limits).unwrap(), maximum);
    assert_eq!(limits.max_queue_age_ms(), 60_000);
}

#[test]
fn ev_09_each_limit_rejects_noninteger_nonpositive_and_above_hard_values() {
    for (key, maximum) in [
        ("max_queued_events", 1_024),
        ("max_queued_bytes", 4_194_304),
        ("max_queue_age_ms", 60_000),
        ("max_retained_bytes", 67_108_864),
        ("max_sequence_bytes", 16_777_216),
    ] {
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(true),
            json!("1"),
            json!(null),
            json!({}),
            json!(maximum + 1),
            json!(u64::MAX),
        ] {
            let input = json!({key: value});
            assert!(
                serde_json::from_value::<SourceLimits>(input).is_err(),
                "{key}"
            );
        }
    }
    assert_eq!(
        SourceLimits::new(0, 1, 1, 1, 1).unwrap_err(),
        EventSettingsError::LimitRange
    );
}

#[test]
fn ev_09_inconsistent_shared_and_definition_limits_fail_without_clamping() {
    assert_eq!(
        SourceLimits::new(1, 1, 1, 1, 2).unwrap_err(),
        EventSettingsError::SequenceRetention
    );
    // A partial override still checks its relationship to the default definition ceiling.
    assert!(serde_json::from_value::<SourceLimits>(json!({"max_retained_bytes": 1})).is_err());
    let limits: SourceLimits = serde_json::from_value(json!({
        "max_retained_bytes": 1, "max_sequence_bytes": 1,
    }))
    .unwrap();
    assert_eq!(limits.max_retained_bytes(), 1);
    assert_eq!(limits.max_sequence_bytes(), 1);
}

#[test]
fn ev_12_settings_are_closed_at_each_owned_data_boundary() {
    for value in [
        json!({"extra": {}}),
        json!({"limits": {"extra": 1}}),
        json!({"consumers": {"audit": {"field": "changed", "enabled": false, "extra": 1}}}),
        json!({"limits": null}),
        json!({"consumers": null}),
        json!({"consumers": []}),
    ] {
        assert!(serde_json::from_value::<EventSettings>(value).is_err());
    }
}
