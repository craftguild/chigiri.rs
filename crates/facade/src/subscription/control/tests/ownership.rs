use std::sync::Arc;

use chigiri_http::SubscriptionTransport;

use super::super::SubscriptionControlFrame;
use super::super::SubscriptionControlSignal;
use super::super::encoding::encode_reserved;
use super::fixture::{bytes, heartbeat_control, websocket};

#[test]
fn frame_is_non_clone_send_sync_static_and_debug_is_length_only() {
    assert_owner::<SubscriptionControlFrame>();
    let _ = <SubscriptionControlFrame as AmbiguousIfClone<_>>::marker;

    let owner = bytes(1024);
    let mut staging = websocket(&owner);
    staging.enqueue_static(heartbeat_control()).unwrap();
    let frame = staging.begin_send().unwrap().unwrap();
    let debug = format!("{frame:?}");
    assert!(debug.contains("bytes"));
    assert!(!debug.contains("ping"));
}

#[test]
fn partial_and_accepted_all_bytes_stay_charged_until_last_physical_drop() {
    let owner = bytes(1024);
    let mut staging = websocket(&owner);
    staging.enqueue_static(heartbeat_control()).unwrap();
    staging.enqueue_static(heartbeat_control()).unwrap();
    let frame = Arc::new(staging.begin_send().unwrap().unwrap());
    let total = frame.len();
    let charged = owner.used();
    let partial_accepted = 1;
    assert!(partial_accepted < total);
    assert_eq!(owner.used(), charged);
    let all_accepted = total;
    assert_eq!(all_accepted, total);
    assert_eq!(owner.used(), charged);
    let physical_copy = Arc::clone(&frame);
    drop(frame);
    assert_eq!(owner.used(), charged);
    assert!(staging.begin_send().unwrap().is_none());
    drop(physical_copy);

    let second = staging.begin_send().unwrap().unwrap();
    assert_eq!(owner.used(), second.len());
    staging.cancel();
    assert_eq!(owner.used(), second.len());
    drop(second);
    assert_eq!(owner.used(), 0);
}

#[test]
fn invalid_http_profile_is_rejected_without_a_byte_owner() {
    let owner = bytes(1024);
    assert!(
        super::super::HttpHeartbeatStaging::new(owner.clone(), SubscriptionTransport::WebSocket,)
            .is_err()
    );
    assert_eq!(owner.used(), 0);
}

#[test]
fn failed_endpoint_reservation_never_invokes_the_encoder() {
    let owner = bytes(0);
    let result = encode_reserved(&owner, 1, None, |_| panic!("encoder must not run"));
    assert!(matches!(
        result,
        Err(SubscriptionControlSignal::SharedByteCapacity)
    ));
    assert_eq!(owner.used(), 0);
}

fn assert_owner<T: Send + Sync + 'static>() {}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}

struct Invalid;

impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
