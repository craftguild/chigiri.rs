use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::Waker;
#[cfg(feature = "subscription-worker")]
use std::time::{Duration, Instant};

use chigiri_http::{RequestPhase, ResponseCompletionOutcome};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SetupMode {
    Ready,
    OuterError,
    Pending,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum EventMode {
    Object,
    Pending,
    FrameworkInvariant,
}

#[derive(Clone, Copy)]
pub(super) enum SourceStep {
    Event(i32),
    Error,
    Pending,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FactoryMode {
    Ready,
    Reject,
    Internal,
    Pending,
    #[cfg(feature = "subscription-worker")]
    Panic,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum HookMode {
    Ready,
    Internal,
    Pending,
}

#[derive(Default)]
pub(super) struct Counts {
    pub(super) setup_calls: AtomicUsize,
    pub(super) nested_calls: AtomicUsize,
    pub(super) source_polls: AtomicUsize,
    pub(super) source_drops: AtomicUsize,
    pub(super) event_drops: AtomicUsize,
    pub(super) setup_future_drops: AtomicUsize,
    pub(super) event_future_drops: AtomicUsize,
    pub(super) source_maps: AtomicUsize,
    pub(super) factory_calls: AtomicUsize,
    pub(super) setup_context_drops: AtomicUsize,
    pub(super) event_context_drops: AtomicUsize,
    pub(super) setup_state_drops: AtomicUsize,
    pub(super) event_state_drops: AtomicUsize,
    pub(super) hook_calls: AtomicUsize,
    pub(super) completion_calls: AtomicUsize,
    pub(super) completion_drops: AtomicUsize,
    pub(super) factory_future_drops: AtomicUsize,
    pub(super) hook_future_drops: AtomicUsize,
    pub(super) request_ids: AtomicUsize,
    pub(super) scalar_calls: AtomicUsize,
}

pub(super) struct State {
    source_observer: Mutex<Option<Waker>>,
    pub(super) source_pending_observed: AtomicBool,
    pub(super) source_pending_instances: AtomicUsize,
    #[cfg(feature = "subscription-worker")]
    pub(super) host_observation: Mutex<Option<Arc<super::observation::Probe>>>,
    pub(super) counts: Counts,
    pub(super) setup_mode: SetupMode,
    pub(super) event_mode: EventMode,
    pub(super) setup_factory: FactoryMode,
    pub(super) event_factory: FactoryMode,
    pub(super) hook_mode: HookMode,
    pub(super) source: Mutex<Vec<SourceStep>>,
    pub(super) phases: Mutex<Vec<RequestPhase>>,
    pub(super) phase_request_ids: Mutex<Vec<Option<String>>>,
    pub(super) hook_error_counts: Mutex<Vec<usize>>,
    pub(super) completion_outcomes: Mutex<Vec<ResponseCompletionOutcome>>,
    pub(super) completion_drop_counts: Mutex<Vec<[usize; 4]>>,
    pub(super) event_generations: Mutex<Vec<usize>>,
    pub(super) retained_arguments: Mutex<Vec<i32>>,
    scalar_gate: Mutex<Option<Arc<ScalarGate>>>,
    #[cfg(feature = "subscription-worker")]
    source_drop_gate: Mutex<Option<Arc<ScalarGate>>>,
}

impl State {
    pub(super) fn new(source: Vec<SourceStep>) -> Arc<Self> {
        Self::modes(
            source,
            SetupMode::Ready,
            EventMode::Object,
            FactoryMode::Ready,
            FactoryMode::Ready,
            HookMode::Ready,
        )
    }

    pub(super) fn modes(
        source: Vec<SourceStep>,
        setup_mode: SetupMode,
        event_mode: EventMode,
        setup_factory: FactoryMode,
        event_factory: FactoryMode,
        hook_mode: HookMode,
    ) -> Arc<Self> {
        Arc::new(Self {
            source_observer: Mutex::new(None),
            source_pending_observed: AtomicBool::new(false),
            source_pending_instances: AtomicUsize::new(0),
            #[cfg(feature = "subscription-worker")]
            host_observation: Mutex::new(None),
            counts: Counts::default(),
            setup_mode,
            event_mode,
            setup_factory,
            event_factory,
            hook_mode,
            source: Mutex::new(source),
            phases: Mutex::new(Vec::new()),
            phase_request_ids: Mutex::new(Vec::new()),
            hook_error_counts: Mutex::new(Vec::new()),
            completion_outcomes: Mutex::new(Vec::new()),
            completion_drop_counts: Mutex::new(Vec::new()),
            event_generations: Mutex::new(Vec::new()),
            retained_arguments: Mutex::new(Vec::new()),
            scalar_gate: Mutex::new(None),
            #[cfg(feature = "subscription-worker")]
            source_drop_gate: Mutex::new(None),
        })
    }

    pub(super) fn record_source_poll(&self, new_pending_source: bool) {
        self.counts.source_polls.fetch_add(1, Ordering::SeqCst);
        if new_pending_source {
            self.source_pending_instances.fetch_add(1, Ordering::SeqCst);
            self.source_pending_observed.store(true, Ordering::SeqCst);
        }
        let observer = self.source_observer.lock().unwrap().take();
        if let Some(observer) = observer {
            observer.wake();
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn observe_source(&self, observer: Option<&Waker>) {
        *self.source_observer.lock().unwrap() = observer.cloned();
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn block_scalar(&self) -> Arc<ScalarGate> {
        let gate = Arc::new(ScalarGate::default());
        *self.scalar_gate.lock().unwrap() = Some(Arc::clone(&gate));
        gate
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn block_source_drop(&self) -> SourceDropBlock {
        let gate = Arc::new(ScalarGate::default());
        *self.source_drop_gate.lock().unwrap() = Some(gate.clone());
        SourceDropBlock(gate)
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn wait_in_source_drop(&self) {
        let gate = self.source_drop_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.enter_and_wait();
        }
    }

    pub(super) fn wait_in_scalar(&self) {
        if let Some(gate) = self.scalar_gate.lock().unwrap().clone() {
            gate.enter_and_wait();
        }
    }
}

#[cfg(feature = "subscription-worker")]
pub(super) struct SourceDropBlock(Arc<ScalarGate>);

#[cfg(feature = "subscription-worker")]
impl SourceDropBlock {
    pub(super) fn wait_entered(&self) {
        assert!(
            self.0.wait_entered(Duration::from_secs(2)),
            "source destructor must enter"
        );
    }

    pub(super) fn release(&self) {
        self.0.release();
    }
}

#[cfg(feature = "subscription-worker")]
impl Drop for SourceDropBlock {
    fn drop(&mut self) {
        self.release();
    }
}

#[derive(Default)]
pub(super) struct ScalarGate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl ScalarGate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn wait_entered(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        let deadline = Instant::now() + timeout;
        while !state.0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let result = self.changed.wait_timeout(state, remaining).unwrap();
            state = result.0;
            if result.1.timed_out() && !state.0 {
                return false;
            }
        }
        true
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = true;
        self.changed.notify_all();
    }
}

impl Drop for ScalarGate {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap();
        state.1 = true;
        self.changed.notify_all();
    }
}

#[derive(Clone, Copy)]
pub(super) enum ContextKind {
    Setup,
    Event,
}

pub(super) struct AppContext {
    pub(super) generation: usize,
    pub(super) kind: ContextKind,
    pub(super) state: Arc<State>,
}

impl Drop for AppContext {
    fn drop(&mut self) {
        let counter = match self.kind {
            ContextKind::Setup => &self.state.counts.setup_context_drops,
            ContextKind::Event => &self.state.counts.event_context_drops,
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct HookState {
    pub(super) kind: ContextKind,
    pub(super) state: Arc<State>,
}

impl Drop for HookState {
    fn drop(&mut self) {
        let counter = match self.kind {
            ContextKind::Setup => &self.state.counts.setup_state_drops,
            ContextKind::Event => &self.state.counts.event_state_drops,
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) fn count(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}
