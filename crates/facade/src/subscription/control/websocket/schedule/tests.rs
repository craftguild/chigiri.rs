use super::{WebSocketControlClass as Class, WebSocketWriterSchedule, WebSocketWriterTurn as Turn};
use crate::subscription::control::SubscriptionControlSignal;

#[test]
fn eight_ordinary_controls_yield_to_one_available_event() {
    let mut schedule = WebSocketWriterSchedule::default();
    for _ in 0..8 {
        assert_eq!(
            schedule.select(&[true], false, true),
            Some(Turn::Control(Class::Ordinary))
        );
        schedule.control_acquired(Class::Ordinary);
    }
    assert_eq!(schedule.select(&[true], false, true), Some(Turn::Event(0)));
    schedule.event_acquired(0, 1).unwrap();
    assert_eq!(
        schedule.select(&[true], false, true),
        Some(Turn::Control(Class::Ordinary))
    );
}

#[test]
fn priority_preempts_without_granting_another_ordinary_control_run() {
    let mut schedule = WebSocketWriterSchedule::default();
    for _ in 0..8 {
        schedule.control_acquired(Class::Ordinary);
    }
    for _ in 0..32 {
        assert_eq!(
            schedule.select(&[true], true, true),
            Some(Turn::Control(Class::Priority))
        );
        schedule.control_acquired(Class::Priority);
    }
    assert_eq!(schedule.select(&[true], false, true), Some(Turn::Event(0)));
}

#[test]
fn idle_sources_never_block_control_and_cannot_reset_an_owed_event_turn() {
    let mut schedule = WebSocketWriterSchedule::default();
    for _ in 0..1_000 {
        assert_eq!(
            schedule.select(&[], false, true),
            Some(Turn::Control(Class::Ordinary))
        );
        schedule.control_acquired(Class::Ordinary);
    }
    assert_eq!(schedule.select(&[], false, false), None);
    assert_eq!(
        schedule.select(&[false, true], false, true),
        Some(Turn::Event(1))
    );
    assert_eq!(schedule.ordinary_run, 8);
}

#[test]
fn sparse_slots_rotate_without_starving_a_newly_ready_operation() {
    let mut schedule = WebSocketWriterSchedule::default();
    for expected in [0, 2, 3, 0, 2, 3] {
        assert_eq!(
            schedule.select(&[true, false, true, true], false, false),
            Some(Turn::Event(expected))
        );
        schedule.event_acquired(expected, 4).unwrap();
    }
    assert_eq!(
        schedule.select(&[false, true, true, true], false, false),
        Some(Turn::Event(1))
    );
    schedule.event_acquired(1, 4).unwrap();
    assert_eq!(schedule.select(&[true], false, false), Some(Turn::Event(0)));
}

#[test]
fn selection_and_failed_acquisition_do_not_advance_the_cursor_or_run() {
    let mut schedule = WebSocketWriterSchedule::default();
    schedule.event_acquired(1, 3).unwrap();
    for _ in 0..8 {
        schedule.control_acquired(Class::Ordinary);
    }
    for _ in 0..32 {
        assert_eq!(
            schedule.select(&[true, true, true], false, true),
            Some(Turn::Event(2))
        );
    }
    assert_eq!(
        schedule.event_acquired(3, 3),
        Err(SubscriptionControlSignal::Accounting)
    );
    assert_eq!(
        schedule.event_acquired(0, 0),
        Err(SubscriptionControlSignal::Accounting)
    );
    assert_eq!(
        schedule.select(&[true, true, true], false, true),
        Some(Turn::Event(2))
    );
    schedule.event_acquired(2, 3).unwrap();
    assert_eq!(
        schedule.select(&[true, true, true], false, false),
        Some(Turn::Event(0))
    );
}
