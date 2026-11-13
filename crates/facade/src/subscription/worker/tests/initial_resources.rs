use chigiri_http::{ResponseMediaType, ResponseParts};

use super::super::{InitialResponseBufferLease, InitialWorkerResources};
use crate::subscription::control::{MAX_INITIAL_RESPONSE_BYTES, reserve_initial_preparation};
use crate::subscription::delivery::EndpointByteOwner;

const LIMIT: usize = MAX_INITIAL_RESPONSE_BYTES + 1024;

fn resources(bytes: &EndpointByteOwner) -> InitialWorkerResources {
    let (inputs, guards) =
        reserve_initial_preparation(bytes, ResponseMediaType::GraphQlResponseJson).unwrap();
    InitialWorkerResources::new(inputs, guards)
}

fn response(body_len: usize) -> ResponseParts {
    ResponseParts {
        status: 200,
        headers: Vec::new(),
        body: vec![b'x'; body_len],
    }
}

#[test]
fn abandoned_outer_lease_keeps_the_buffer_in_retained_storage() {
    let bytes = EndpointByteOwner::new(LIMIT);
    let mut retained = resources(&bytes);
    let admitted = bytes.used();
    let lease = retained.take_response_buffer().unwrap();
    assert!(retained.take_response_buffer().is_none());
    drop(lease);
    assert_eq!(bytes.used(), admitted);
    drop(retained);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn exact_wire_handoff_and_fallback_charge_release_are_independent() {
    let bytes = EndpointByteOwner::new(LIMIT);
    let mut retained = resources(&bytes);
    let task_inputs = retained.take_task_inputs().unwrap();
    let lease = retained.take_response_buffer().unwrap();
    let response = response(23);
    let wire = lease.finish(&response).unwrap();
    assert!(bytes.used() > response.body.len());
    retained.release_fallbacks();
    assert_eq!(bytes.used(), response.body.len());
    drop((task_inputs, retained, response));
    assert_eq!(bytes.used(), 23);
    drop(wire);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn finite_transition_releases_callback_inputs_but_preserves_maximum_buffer() {
    let bytes = EndpointByteOwner::new(LIMIT);
    let lease = resources(&bytes).into_finite_buffer().unwrap();
    assert_eq!(bytes.used(), MAX_INITIAL_RESPONSE_BYTES);
    let response = response(7);
    let wire = lease.finish(&response).unwrap();
    assert_eq!(bytes.used(), 7);
    drop((response, wire));
    assert_eq!(bytes.used(), 0);
}

#[test]
fn invalid_outer_response_keeps_the_maximum_for_a_bounded_fallback() {
    let bytes = EndpointByteOwner::new(LIMIT);
    let mut lease = resources(&bytes).into_finite_buffer().unwrap();
    assert!(
        lease
            .try_finish(&response(MAX_INITIAL_RESPONSE_BYTES + 1))
            .is_err()
    );
    assert_eq!(bytes.used(), MAX_INITIAL_RESPONSE_BYTES);
    assert!(lease.retains_maximum());
    let wire = lease.try_finish(&response(11)).unwrap();
    assert!(!lease.retains_maximum());
    drop(lease);
    assert_eq!(bytes.used(), 11);
    drop(wire);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn lease_is_send_sync_static_and_not_clone() {
    fn assert_owner<T: Send + Sync + 'static>() {}
    assert_owner::<InitialResponseBufferLease>();
    let _ = <InitialResponseBufferLease as AmbiguousIfClone<_>>::marker;
}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}
impl<T: ?Sized> AmbiguousIfClone<()> for T {}
struct Invalid;
impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
