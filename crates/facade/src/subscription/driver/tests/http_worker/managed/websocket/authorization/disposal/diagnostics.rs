//! Closed numeric observations identify a bounded fixture's failed transition.

use std::cell::{Cell, RefCell};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{AppContext, Endpoint, Probe, count};

const OUTPUT: &str = "CHIGIRI_AUTHORIZATION_TEST_PANIC_OUTPUT";
const PHASES: &[&str] = &[
    "managed opening",
    "authorization entry",
    "authorization observation",
    "shutdown completion",
    "actual native join",
];

pub(super) fn child(case: &str, output: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args([super::TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(super::CHILD, case)
        .env(OUTPUT, output)
        .output()
        .unwrap()
}

pub(super) fn observe_panics() -> Arc<AtomicUsize> {
    let mode = std::env::var(OUTPUT).unwrap_or_else(|_| "bounded".into());
    assert!(matches!(mode.as_str(), "bounded" | "forward" | "delay"));
    let original = std::panic::take_hook();
    let observed = Arc::new(AtomicUsize::new(0));
    let count = observed.clone();
    // Only this isolated child changes its diagnostic hook. Panic/unwind,
    // destructor behavior and every production deadline remain unchanged.
    std::panic::set_hook(Box::new(move |info| {
        let first = count.fetch_add(1, Ordering::SeqCst) == 0;
        if mode == "forward" {
            let started = Instant::now();
            original(info);
            eprintln!(
                "authorization default panic output elapsed: {:?}",
                started.elapsed()
            );
        } else if mode == "delay" && first {
            eprintln!("authorization diagnostic delay probe entered");
            std::thread::sleep(Duration::from_millis(3500));
        }
    }));
    observed
}

struct Transition {
    previous: Option<&'static str>,
    next: &'static str,
    elapsed: Duration,
    observation: String,
}

pub(super) struct Evidence<'a> {
    endpoint: &'a Endpoint<AppContext>,
    probe: &'a Probe,
    phase: Cell<&'static str>,
    completed: Cell<bool>,
    case: String,
    started: Instant,
    history: RefCell<Vec<Transition>>,
}

impl<'a> Evidence<'a> {
    pub(super) fn new(endpoint: &'a Endpoint<AppContext>, probe: &'a Probe, case: &str) -> Self {
        Self {
            endpoint,
            probe,
            phase: Cell::new("not polled"),
            completed: Cell::new(false),
            case: case.into(),
            started: Instant::now(),
            history: RefCell::new(vec![Transition {
                previous: None,
                next: "not polled",
                elapsed: Duration::ZERO,
                observation: "initial".into(),
            }]),
        }
    }

    pub(super) fn at(&self, phase: &'static str) {
        let mut history = self.history.borrow_mut();
        assert_eq!(PHASES.get(history.len() - 1), Some(&phase));
        history.push(Transition {
            previous: Some(self.phase.replace(phase)),
            next: phase,
            elapsed: self.started.elapsed(),
            observation: format!(
                "calls={}, errors={}, drops={}, connections={:?}, physical={:?}",
                count(&self.probe.calls),
                count(&self.probe.errors),
                count(&self.probe.drops),
                self.endpoint.subscription_connection_counts_for_test(),
                self.endpoint.subscription_physical_counts_for_test(),
            ),
        });
    }

    pub(super) fn complete(&self) {
        self.completed.set(true);
    }
}

impl Drop for Evidence<'_> {
    fn drop(&mut self) {
        if !self.completed.get() {
            for (sequence, transition) in self.history.borrow().iter().enumerate() {
                eprintln!(
                    "authorization transition: case={}, sequence={}, previous={:?}, next={}, elapsed={:?}, {}",
                    self.case,
                    sequence,
                    transition.previous,
                    transition.next,
                    transition.elapsed,
                    transition.observation,
                );
            }
            eprintln!(
                "authorization fixture incomplete: phase={}, calls={}, errors={}, drops={}, outcome={:?}, connections={:?}, physical={:?}, process_poisoned={}",
                self.phase.get(),
                count(&self.probe.calls),
                count(&self.probe.errors),
                count(&self.probe.drops),
                self.endpoint.shutdown_outcome(),
                self.endpoint.subscription_connection_counts_for_test(),
                self.endpoint.subscription_physical_counts_for_test(),
                crate::subscription::worker::process_poisoned_for_test(),
            );
        }
    }
}
