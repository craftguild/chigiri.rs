use std::sync::Arc;
use std::task::Poll;

use chigiri_runtime::__private::{SubscriptionEventResult, SubscriptionSourceState};

use super::fixture::{
    AppContext, EventMode, SetupMode, SourceStep, State, block_on, count, poll_once, request,
    schema, start,
};

#[test]
fn dropping_start_before_and_after_first_poll_releases_owned_work() {
    let before = State::modes(Vec::new(), SetupMode::Pending, EventMode::Object);
    let before_schema = schema(Arc::clone(&before));
    let context = AppContext::new(0);
    let prepared = before_schema.prepare_subscription(request()).unwrap();
    let future = prepared.start(&context, None);
    drop(future);
    assert_eq!(count(&before.counts.setup_calls), 0);
    assert_eq!(count(&before.counts.setup_future_drops), 0);

    let after = State::modes(Vec::new(), SetupMode::Pending, EventMode::Object);
    let after_schema = schema(Arc::clone(&after));
    let prepared = after_schema.prepare_subscription(request()).unwrap();
    let mut future = prepared.start(&context, None);
    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    assert_eq!(count(&after.counts.setup_calls), 1);
    assert_eq!(count(&after.counts.setup_future_drops), 0);
    drop(future);
    assert_eq!(count(&after.counts.setup_future_drops), 1);
}

#[test]
fn dropping_a_pending_source_poll_releases_the_source_and_cancels_the_session() {
    let state = State::new(vec![SourceStep::Pending]);
    let schema = schema(Arc::clone(&state));
    let context = AppContext::new(0);
    let mut session = start(&schema, &context);
    let mut future = session.next_source(None);
    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    assert_eq!(count(&state.counts.source_polls), 1);
    assert_eq!(count(&state.counts.source_drops), 0);
    drop(future);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

#[test]
fn dropping_an_unpolled_source_future_releases_without_polling_or_mapping() {
    let state = State::new(vec![SourceStep::Error]);
    let schema = schema(Arc::clone(&state));
    let context = AppContext::new(0);
    let mut session = start(&schema, &context);

    let future = session.next_source(None);
    drop(future);

    assert_eq!(count(&state.counts.source_polls), 0);
    assert_eq!(count(&state.counts.source_maps), 0);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

#[test]
fn dropping_a_pending_event_releases_event_source_and_future_without_resume() {
    let state = State::modes(
        vec![SourceStep::Event(5)],
        SetupMode::Ready,
        EventMode::Pending,
    );
    let schema = schema(Arc::clone(&state));
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    let event = AppContext::new(1);
    let mut future = session.execute_event(&event, None, None);
    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    assert_eq!(count(&state.counts.source_polls), 1);
    assert_eq!(count(&state.counts.event_future_drops), 0);
    drop(future);

    assert_eq!(count(&state.counts.event_future_drops), 1);
    assert_eq!(count(&state.counts.event_drops), 1);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert!(matches!(
        block_on(session.execute_event(&event, None, None)),
        SubscriptionEventResult::Cancelled
    ));
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

#[test]
fn dropping_an_unpolled_event_future_releases_owned_work_without_dispatch() {
    let state = State::modes(
        vec![SourceStep::Event(5)],
        SetupMode::Ready,
        EventMode::Pending,
    );
    let schema = schema(Arc::clone(&state));
    let setup = AppContext::new(0);
    let mut session = start(&schema, &setup);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));

    let event = AppContext::new(1);
    let future = session.execute_event(&event, None, None);
    drop(future);

    assert_eq!(count(&state.counts.event_dispatches), 0);
    assert_eq!(count(&state.counts.event_maps), 0);
    assert_eq!(count(&state.counts.event_future_drops), 0);
    assert_eq!(count(&state.counts.event_drops), 1);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert!(state.event_generations.lock().unwrap().is_empty());
    assert!(matches!(
        block_on(session.execute_event(&event, None, None)),
        SubscriptionEventResult::Cancelled
    ));
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}

#[test]
fn synchronous_cancel_drops_both_a_ready_event_and_its_source() {
    let state = State::new(vec![SourceStep::Event(6)]);
    let schema = schema(Arc::clone(&state));
    let context = AppContext::new(0);
    let mut session = start(&schema, &context);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::EventReady
    ));
    assert_eq!(count(&state.counts.event_drops), 0);
    assert_eq!(count(&state.counts.source_drops), 0);

    session.cancel();

    assert_eq!(count(&state.counts.event_drops), 1);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert!(matches!(
        block_on(session.next_source(None)),
        SubscriptionSourceState::Cancelled
    ));
}
