//! Exercise the fixed decision slot without minting a runtime receipt in Mock.

use std::sync::{Arc, Barrier, Mutex};

use chigiri::__private::SubscriptionTermination;
use chigiri::SubscriptionOverflow;

use super::{Record, State};

#[test]
fn cancellation_before_entry_is_silent_and_closes_late_work() {
    let mut state = State::new();
    state.finish(SubscriptionTermination::ClientCancelled);

    let records = state.drain(0);
    assert!(records.first.is_none() && records.second.is_none());
    assert!(!state.enter());
    assert!(!state.register(1, SubscriptionOverflow::Backpressure));
    assert!(state.drain(0).first.is_none());
}

#[test]
fn entered_failure_emits_one_nullable_setup_and_no_terminal() {
    let mut state = State::new();
    assert!(state.enter());
    state.finish(SubscriptionTermination::ExecutionFailed);

    assert!(matches!(
        state.drain(0).first,
        Some(Record::SetupUnregistered { cancelled: false })
    ));
    let duplicate = state.drain(0);
    assert!(duplicate.first.is_none() && duplicate.second.is_none());
    assert!(!state.register(1, SubscriptionOverflow::Backpressure));
}

#[test]
fn registration_orders_setup_before_terminal_and_freezes_drop_total() {
    let mut state = State::new();
    assert!(state.enter());
    assert!(state.register(7, SubscriptionOverflow::DropOldest));
    assert!(state.registered(SubscriptionOverflow::DropOldest));
    assert!(!state.registered(SubscriptionOverflow::Backpressure));
    assert_eq!(state.drop_oldest(1), Some(7));
    assert_eq!(state.drop_oldest(1), None);
    state.finish(SubscriptionTermination::SourceComplete);
    state.finish(SubscriptionTermination::Internal);
    assert_eq!(state.drop_oldest(2), None);

    let records = state.drain(1);
    assert!(matches!(
        records.first,
        Some(Record::SetupRegistered {
            generation: 7,
            overflow: SubscriptionOverflow::DropOldest
        })
    ));
    assert!(matches!(
        records.second,
        Some(Record::Terminal {
            generation: 7,
            decision: SubscriptionTermination::SourceComplete,
            dropped_total: 1
        })
    ));
    let duplicate = state.drain(1);
    assert!(duplicate.first.is_none() && duplicate.second.is_none());
}

#[test]
fn registration_and_terminal_race_select_exactly_one_valid_history() {
    let state = Arc::new(Mutex::new(State::new()));
    assert!(state.lock().unwrap().enter());
    let ready = Arc::new(Barrier::new(3));

    let register_state = Arc::clone(&state);
    let register_ready = Arc::clone(&ready);
    let register = std::thread::spawn(move || {
        register_ready.wait();
        register_state
            .lock()
            .unwrap()
            .register(9, SubscriptionOverflow::Backpressure)
    });
    let finish_state = Arc::clone(&state);
    let finish_ready = Arc::clone(&ready);
    let finish = std::thread::spawn(move || {
        finish_ready.wait();
        finish_state
            .lock()
            .unwrap()
            .finish(SubscriptionTermination::ServerShutdown);
    });
    ready.wait();
    let registered = register.join().unwrap();
    finish.join().unwrap();

    let mut state = state.lock().unwrap();
    let records = state.drain(0);
    if registered {
        assert!(matches!(
            records.first,
            Some(Record::SetupRegistered { generation: 9, .. })
        ));
        assert!(matches!(
            records.second,
            Some(Record::Terminal { generation: 9, .. })
        ));
    } else {
        assert!(matches!(
            records.first,
            Some(Record::SetupUnregistered { cancelled: true })
        ));
        assert!(records.second.is_none());
    }
    assert!(!state.enter());
    assert!(!state.register(10, SubscriptionOverflow::Backpressure));
}
