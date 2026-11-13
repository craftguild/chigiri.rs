//! Serialize access to pinned luaur process-global state for each VM's complete lifetime.

use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

static GATE: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Cancelled,
    Expired,
    Poisoned,
}

/// Wait for VM admission under the caller's existing absolute deadline and cancellation signal.
pub(crate) fn enter(
    deadline: Instant,
    cancelled: impl Fn() -> bool,
) -> Result<MutexGuard<'static, ()>, AdmissionError> {
    enter_gate(&GATE, deadline, cancelled)
}

/// A panicked VM cannot be safely admitted again in this process.
pub(crate) fn poisoned() -> bool {
    GATE.is_poisoned()
}

fn enter_gate(
    gate: &Mutex<()>,
    deadline: Instant,
    cancelled: impl Fn() -> bool,
) -> Result<MutexGuard<'_, ()>, AdmissionError> {
    loop {
        if cancelled() {
            return Err(AdmissionError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(AdmissionError::Expired);
        }
        match gate.try_lock() {
            Ok(guard) => {
                // Resolve a simultaneous release before admitting the first waiting checker.
                luaur::common::set_all_flags(true);
                return Ok(guard);
            }
            Err(TryLockError::Poisoned(_)) => return Err(AdmissionError::Poisoned),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

#[cfg(test)]
pub(crate) fn test_guard() -> MutexGuard<'static, ()> {
    enter(Instant::now() + Duration::from_secs(30), || false).unwrap()
}

#[cfg(test)]
pub(crate) struct TestCase {
    _guard: MutexGuard<'static, ()>,
}

/// Permit a dedicated test worker to bypass the process-wide production gate.
/// Serialize these VM-heavy cases separately because unrelated CPU contention can exhaust production deadlines.
#[cfg(test)]
pub(crate) fn test_case() -> TestCase {
    static CASES: Mutex<()> = Mutex::new(());
    TestCase {
        // Assertion failure is recorded by the libtest.
        _guard: CASES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    }
}

#[test]
fn rsm_15_17_gate_wait_keeps_deadline_cancel_and_next_acquisition() {
    let _case = test_case();
    let lease = test_guard();
    let thread = std::thread::spawn(|| {
        let start = Instant::now();
        assert!(enter(start + Duration::from_millis(30), || false).is_err());
        assert!(start.elapsed() < Duration::from_millis(200));
    });
    thread.join().unwrap();
    let cancel = crate::cancellation::Cancellation::default();
    let other = cancel.clone();
    let (ready, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        ready.send(()).unwrap();
        assert!(
            enter(Instant::now() + Duration::from_secs(1), || other
                .cancelled())
            .is_err()
        );
    });
    receive.recv().unwrap();
    let start = Instant::now();
    cancel.cancel();
    thread.join().unwrap();
    assert!(start.elapsed() < Duration::from_millis(200));
    drop(lease);
    assert!(enter(Instant::now() + Duration::from_millis(100), || false).is_ok());
}

#[test]
fn admission_failure_distinguishes_cancellation_deadline_and_poisoning() {
    let gate = Mutex::new(());
    let future = Instant::now() + Duration::from_secs(1);
    assert_eq!(
        enter_gate(&gate, future, || true).unwrap_err(),
        AdmissionError::Cancelled
    );
    assert_eq!(
        enter_gate(&gate, Instant::now() - Duration::from_millis(1), || false).unwrap_err(),
        AdmissionError::Expired
    );
    let _ = std::panic::catch_unwind(|| {
        let _guard = gate.lock().unwrap();
        panic!("poison test-case gate");
    });
    assert_eq!(
        enter_gate(&gate, future, || false).unwrap_err(),
        AdmissionError::Poisoned
    );
}
