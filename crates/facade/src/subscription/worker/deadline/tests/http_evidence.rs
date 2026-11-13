//! HTTP phase classification follows the exact timer generation at expiry.

use super::*;
use crate::subscription::worker::HttpPhaseDeadline;
use chigiri_runtime::LifecyclePhase;

fn phase(kind: LifecyclePhase) -> HttpPhaseDeadline {
    HttpPhaseDeadline {
        kind,
        cutoff: tokio::time::Instant::now(),
    }
}

#[test]
fn fired_phase_evidence_survives_clear_and_rearm_of_the_slot() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let body = phase(LifecyclePhase::BodyRead);
    let previous = state.arm_http_phase(Box::pin(async {}), body).unwrap();
    let Poll::Ready(Ok(fired)) = poll(&state) else {
        panic!("body deadline must fire");
    };
    previous.clear().unwrap();
    let next = state
        .arm_http_phase(
            Box::pin(std::future::pending()),
            phase(LifecyclePhase::OperationSourceResolution),
        )
        .unwrap();
    assert_eq!(fired.http_phase, Some(body));
    next.clear().unwrap();
}

#[test]
fn completed_phase_does_not_change_a_later_overall_timeout() {
    let (overall, gate) = blocked_timer(true);
    let state = DeadlineState::with_overall(overall).unwrap();
    let previous = state
        .arm_http_phase(
            Box::pin(std::future::pending()),
            phase(LifecyclePhase::BodyRead),
        )
        .unwrap();
    let (expiry, cleared, _) = during_poll(&state, gate, || previous.clear());
    cleared.unwrap();
    let Poll::Ready(Ok(fired)) = expiry else {
        panic!("overall deadline must fire");
    };
    assert_eq!(fired.role, DeadlineRole::Overall);
    assert_eq!(fired.http_phase, None);
}

#[test]
fn overall_expiry_captures_only_the_active_replacement_phase() {
    let (overall, gate) = blocked_timer(true);
    let state = DeadlineState::with_overall(overall).unwrap();
    let previous = state
        .arm_http_phase(
            Box::pin(std::future::pending()),
            phase(LifecyclePhase::BodyRead),
        )
        .unwrap();
    let source = phase(LifecyclePhase::OperationSourceResolution);
    let (expiry, next, _) = during_poll(&state, gate, || {
        previous.clear().unwrap();
        state.arm_http_phase(Box::pin(std::future::pending()), source)
    });
    let Poll::Ready(Ok(fired)) = expiry else {
        panic!("overall deadline must fire");
    };
    assert_eq!(fired.role, DeadlineRole::Overall);
    assert_eq!(fired.http_phase, Some(source));
    next.unwrap().clear().unwrap();
}
