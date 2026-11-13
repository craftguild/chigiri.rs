use chigiri_http::{
    __subscription_static_control_len, SubscriptionStaticControl, SubscriptionTransport,
};

use super::super::SubscriptionControlSignal;
use super::fixture::{bytes, heartbeat};

#[test]
fn repeated_timers_coalesce_and_event_or_physical_entry_has_priority() {
    for transport in [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        let owner = bytes(1024);
        let mut staging = heartbeat(&owner, transport);
        staging.mark_due().unwrap();
        staging.mark_due().unwrap();
        assert!(staging.is_pending());
        assert!(staging.try_begin(true, false).unwrap().is_none());
        assert!(staging.is_pending());
        assert!(staging.try_begin(false, true).unwrap().is_none());
        assert!(staging.is_pending());
        assert_eq!(owner.used(), 0);

        let frame = staging.try_begin(false, false).unwrap().unwrap();
        let expected =
            __subscription_static_control_len(transport, SubscriptionStaticControl::Heartbeat)
                .unwrap();
        assert_eq!(frame.len(), expected);
        assert_eq!(owner.used(), expected);
        assert!(!staging.is_pending());

        staging.mark_due().unwrap();
        assert!(staging.try_begin(false, false).unwrap().is_none());
        assert!(staging.is_pending());
        drop(frame);
        let next = staging.try_begin(false, false).unwrap().unwrap();
        assert_eq!(owner.used(), next.len());
        drop(next);
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn reservation_failure_is_terminal_and_never_builds_a_frame() {
    let transport = SubscriptionTransport::ServerSentEvents;
    let required =
        __subscription_static_control_len(transport, SubscriptionStaticControl::Heartbeat).unwrap();
    let owner = bytes(required - 1);
    let mut staging = heartbeat(&owner, transport);
    staging.mark_due().unwrap();
    assert!(matches!(
        staging.try_begin(false, false),
        Err(SubscriptionControlSignal::SharedByteCapacity)
    ));
    assert_eq!(owner.used(), 0);
    assert!(!staging.is_pending());
    assert_eq!(
        staging.mark_due(),
        Err(SubscriptionControlSignal::SharedByteCapacity)
    );
}

#[test]
fn cancellation_clears_only_the_marker_and_not_a_physical_frame() {
    let owner = bytes(1024);
    let mut staging = heartbeat(&owner, SubscriptionTransport::Multipart);
    staging.mark_due().unwrap();
    let frame = staging.try_begin(false, false).unwrap().unwrap();
    let charged = frame.len();
    assert_eq!(owner.used(), charged);
    staging.mark_due().unwrap();
    staging.cancel();
    assert!(!staging.is_pending());
    assert_eq!(owner.used(), charged);
    assert!(matches!(
        staging.try_begin(false, false),
        Err(SubscriptionControlSignal::Cancelled)
    ));
    drop(frame);
    assert_eq!(owner.used(), 0);
}
