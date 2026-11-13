//! Owned worker handoff, bounded ingress, and no-reset deadline assertions.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, DeadlineFactory, RequestHeader, RequestMethod,
    ResponseMediaType, SubscriptionTransport, TransportRequest,
};
use chigiri_runtime::GraphQlRequest;

use super::super::SubscriptionInitialization;
use super::super::input::{SubscriptionPreparation, capture};
use super::deadlines::{Deadlines, HOOK_TIMEOUT, REQUEST_TIMEOUT, SETUP_TIMEOUT};
use super::harness::{block_on, endpoint, invalid_request, request};
use super::state::{AppContext, SourceStep, State, count};
use crate::subscription::bridge_resources::BridgeOperationKind;
use crate::transport::EndpointState;

static DEADLINES: Deadlines = Deadlines(Vec::new());

fn owned_seed<'deadline>(
    state: &Arc<EndpointState<AppContext>>,
    request: GraphQlRequest,
    deadlines: &'deadline dyn DeadlineFactory,
    overall: Option<BoxFuture<'deadline, ()>>,
) -> Result<
    SubscriptionPreparation<'deadline, AppContext>,
    Box<SubscriptionInitialization<AppContext>>,
> {
    let method = String::from("POST");
    let path = String::from("/graphql");
    let query = String::from("private=query");
    let authorization = b"original-secret".to_vec();
    let header_name = String::from("authorization");
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new(&header_name, &authorization)];
    let transport = TransportRequest::new(&method, &path, Some(&query), &headers, None, &metadata);
    capture(
        Arc::clone(&state.subscription_execution),
        transport,
        request,
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        SubscriptionTransport::ServerSentEvents,
        None,
        deadlines,
        overall,
    )
}

fn assert_send_static<T: Send + 'static>(value: T) -> T {
    value
}

#[test]
fn owned_input_and_prepared_result_outlive_buffers_without_retaining_endpoint() {
    let probe = State::new(vec![SourceStep::Event(7)]);
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let weak = Arc::downgrade(&state);
    let seed = assert_send_static(
        owned_seed(&state, request(), &DEADLINES, None)
            .ok()
            .unwrap(),
    );
    assert_eq!(count(&probe.counts.request_ids), 0);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    drop(state);
    assert!(weak.upgrade().is_none());
    let prepared = assert_send_static(seed.prepare(None).ok().unwrap());
    assert_eq!(count(&probe.counts.request_ids), 1);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert!(matches!(
        block_on(prepared.initialize()),
        SubscriptionInitialization::Ready { .. }
    ));
    assert_eq!(count(&probe.counts.request_ids), 1);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_polls), 0);
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn joined_preparation_can_transfer_admission_before_any_application_setup() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let pending = state
        .subscriptions
        .resources
        .begin_http_subscription()
        .unwrap();
    let seed = owned_seed(&state, request(), &DEADLINES, None)
        .ok()
        .unwrap();
    let prepared = seed.prepare(None).ok().unwrap();
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    let active = pending.activate(BridgeOperationKind::Subscription).unwrap();
    assert!(matches!(
        block_on(prepared.initialize()),
        SubscriptionInitialization::Ready { .. }
    ));
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.setup_calls), 1);
    drop(active);
}

#[test]
fn raw_lower_bound_and_prepared_charge_both_reject_before_context_or_source() {
    let raw = State::new(Vec::new());
    let state = endpoint(Arc::clone(&raw), 1);
    assert!(owned_seed(&state, request(), &DEADLINES, None).is_err());
    assert_eq!(count(&raw.counts.request_ids), 0);
    assert_eq!(count(&raw.counts.factory_calls), 0);

    let final_charge = State::new(Vec::new());
    let state = endpoint(Arc::clone(&final_charge), 512);
    let seed = owned_seed(&state, request(), &DEADLINES, None)
        .ok()
        .unwrap();
    assert!(seed.prepare(None).is_err());
    assert_eq!(count(&final_charge.counts.request_ids), 1);
    assert_eq!(count(&final_charge.counts.factory_calls), 0);
    assert_eq!(count(&final_charge.counts.setup_calls), 0);

    let invalid = State::new(Vec::new());
    let state = endpoint(Arc::clone(&invalid), 16 * 1024);
    let seed = owned_seed(&state, invalid_request(), &DEADLINES, None)
        .ok()
        .unwrap();
    assert!(seed.prepare(None).is_err());
    assert_eq!(count(&invalid.counts.factory_calls), 0);
    assert_eq!(count(&invalid.counts.setup_calls), 0);
}

#[test]
fn dropping_either_owned_checkpoint_never_initializes_a_source() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    drop(
        owned_seed(&state, request(), &DEADLINES, None)
            .ok()
            .unwrap(),
    );
    assert_eq!(count(&probe.counts.request_ids), 0);
    let seed = owned_seed(&state, request(), &DEADLINES, None)
        .ok()
        .unwrap();
    drop(seed.prepare(None).ok().unwrap());
    assert_eq!(count(&probe.counts.request_ids), 1);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert_eq!(count(&probe.counts.hook_calls), 0);
}

#[derive(Default)]
struct Clock {
    milliseconds: Arc<AtomicU64>,
    starts: Mutex<Vec<Duration>>,
}

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        self.starts.lock().unwrap().push(duration);
        let end =
            self.milliseconds.load(Ordering::SeqCst) + u64::try_from(duration.as_millis()).unwrap();
        let now = Arc::clone(&self.milliseconds);
        Box::pin(std::future::poll_fn(move |_| {
            if now.load(Ordering::SeqCst) >= end {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

#[test]
fn original_setup_and_overall_deadlines_survive_both_handoffs_without_restart() {
    for inherited in [false, true] {
        let probe = State::new(Vec::new());
        let state = endpoint(Arc::clone(&probe), 16 * 1024);
        let clock = Clock::default();
        let overall = inherited.then(|| clock.sleep(Duration::from_millis(5)));
        clock.milliseconds.store(2, Ordering::SeqCst);
        let seed = owned_seed(&state, request(), &clock, overall).ok().unwrap();
        let expected = if inherited {
            vec![Duration::from_millis(5), SETUP_TIMEOUT]
        } else {
            vec![REQUEST_TIMEOUT, SETUP_TIMEOUT]
        };
        assert_eq!(*clock.starts.lock().unwrap(), expected);
        clock.milliseconds.store(3, Ordering::SeqCst);
        let prepared = seed.prepare(None).ok().unwrap();
        assert_eq!(*clock.starts.lock().unwrap(), expected);
        clock
            .milliseconds
            .store(if inherited { 6 } else { 17 }, Ordering::SeqCst);
        let SubscriptionInitialization::Rejected(response) = block_on(prepared.initialize()) else {
            panic!("an expired original timer must reject before fresh context construction");
        };
        assert_eq!(response.status, 504);
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
    }
}

#[test]
fn setup_timer_remains_authoritative_with_a_longer_inherited_overall_timer() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let clock = Clock::default();
    let overall = clock.sleep(Duration::from_secs(1));
    let seed = owned_seed(&state, request(), &clock, Some(overall))
        .ok()
        .unwrap();
    let prepared = seed.prepare(None).ok().unwrap();
    clock.milliseconds.store(16, Ordering::SeqCst);
    let SubscriptionInitialization::Rejected(response) = block_on(prepared.initialize()) else {
        panic!("setup budget must include preparation and the admission handoff");
    };
    assert_eq!(response.status, 504);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
}

#[test]
fn validation_input_has_no_deadline_owner_and_outer_deadlines_stay_pollable() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let clock = Clock::default();
    let seed = owned_seed(&state, request(), &clock, None).ok().unwrap();
    let (validation, mut timers) = seed.into_validation();
    let validation = assert_send_static(validation);
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(timers.poll_expired(&mut context), Poll::Pending);
    clock.milliseconds.store(16, Ordering::SeqCst);
    assert_eq!(timers.poll_expired(&mut context), Poll::Ready(()));
    assert_eq!(
        *clock.starts.lock().unwrap(),
        [REQUEST_TIMEOUT, SETUP_TIMEOUT]
    );
    assert_eq!(count(&probe.counts.request_ids), 0);
    drop(validation);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
}

#[test]
fn joined_validation_reattaches_the_same_started_timers_without_recreation() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let clock = Clock::default();
    let seed = owned_seed(&state, request(), &clock, None).ok().unwrap();
    let (validation, timers) = seed.into_validation();
    let validated = assert_send_static(validation.prepare().ok().unwrap());
    clock.milliseconds.store(16, Ordering::SeqCst);
    let prepared = validated.with_deadlines(timers);
    assert_eq!(
        *clock.starts.lock().unwrap(),
        [REQUEST_TIMEOUT, SETUP_TIMEOUT]
    );
    let SubscriptionInitialization::Rejected(response) = block_on(prepared.initialize()) else {
        panic!("joining validation must not renew the original budget");
    };
    assert_eq!(response.status, 504);
    assert_eq!(count(&probe.counts.factory_calls), 0);
}

struct ExpireAtHook(Clock);

impl DeadlineFactory for ExpireAtHook {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        if duration == HOOK_TIMEOUT {
            self.0.milliseconds.store(16, Ordering::SeqCst);
        }
        self.0.sleep(duration)
    }
}

#[test]
fn original_setup_deadline_also_bounds_initial_http_hook_preparation() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), 16 * 1024);
    let clock = ExpireAtHook(Clock::default());
    let overall = clock.sleep(Duration::from_secs(1));
    let seed = owned_seed(&state, request(), &clock, Some(overall))
        .ok()
        .unwrap();
    let prepared = seed.prepare(None).ok().unwrap();
    let SubscriptionInitialization::Rejected(response) = block_on(prepared.initialize()) else {
        panic!("source readiness must not end the original setup deadline");
    };
    assert_eq!(response.status, 504);
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(
        *probe.completion_outcomes.lock().unwrap(),
        [chigiri_http::ResponseCompletionOutcome::TimedOut]
    );
}
