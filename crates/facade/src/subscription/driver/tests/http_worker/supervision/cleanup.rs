use std::sync::Arc;
use std::task::Poll;

use chigiri_http::SubscriptionTransport;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{drain, profiles, runtime};
use super::support::{
    active_endpoint, attach, drive_until, drive_until_sources_pending, poll_body_once,
};
use crate::transport::streaming::supervisor::HttpStreamingError;

#[test]
fn dropping_the_body_cancels_the_real_source_and_registration_drains() {
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, _) = attach(
        &runtime,
        Arc::clone(&probe),
        SubscriptionTransport::ServerSentEvents,
    );
    let bytes = live
        .endpoint
        .subscriptions
        .resources
        .delivery()
        .byte_owner();
    drive_until_sources_pending(&runtime, &mut live.driver, &probe, 1);

    drop(live.body);
    drive_until(&runtime, &mut live.driver, || {
        count(&probe.counts.source_drops) == 1
    });
    live.registration.close();
    super::super::support::run(&runtime, live.driver.as_mut()).unwrap();
    drain(&runtime, live.guard);
    assert_eq!(active_endpoint(&live.endpoint), 0);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn dropping_the_driver_cancels_source_but_the_body_keeps_physical_quota() {
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, _) = attach(
        &runtime,
        Arc::clone(&probe),
        SubscriptionTransport::Multipart,
    );
    let bytes = live
        .endpoint
        .subscriptions
        .resources
        .delivery()
        .byte_owner();
    drive_until_sources_pending(&runtime, &mut live.driver, &probe, 1);

    drop(live.driver);
    assert_eq!(active_endpoint(&live.endpoint), 1);
    assert!(bytes.used() > 0);
    assert!(matches!(
        poll_body_once(&runtime, &mut live.body),
        Poll::Ready(None)
    ));
    drop(live.body);
    drop(live.registration);
    drain(&runtime, live.guard);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(active_endpoint(&live.endpoint), 0);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn fatal_registry_failure_reaches_the_body_and_cleans_up_the_source() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let (mut live, _) = attach(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();
        drive_until_sources_pending(&runtime, &mut live.driver, &probe, 1);

        live.driver.as_ref().get_ref().poison_registry_for_test();
        assert_eq!(
            super::super::support::run(&runtime, live.driver.as_mut()),
            Err(HttpStreamingError::Invariant)
        );
        assert_eq!(active_endpoint(&live.endpoint), 1);
        assert!(matches!(
            poll_body_once(&runtime, &mut live.body),
            Poll::Ready(Some(Err(HttpStreamingError::Invariant)))
        ));

        drop(live.body);
        drop(live.registration);
        drain(&runtime, live.guard);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(active_endpoint(&live.endpoint), 0);
        assert_eq!(bytes.used(), 0);
    }
}
