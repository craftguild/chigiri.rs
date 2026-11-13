use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum SetupMode {
    Ready,
    OuterError,
    Pending,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum EventMode {
    Object,
    EventError,
    FrameworkInvariant,
    Pending,
}

#[derive(Clone, Copy)]
pub enum SourceStep {
    Event(i32),
    Error,
    Pending,
}

#[derive(Default)]
pub struct Counts {
    pub setup_calls: AtomicUsize,
    pub ordinary_root_calls: AtomicUsize,
    pub nested_calls: AtomicUsize,
    pub source_polls: AtomicUsize,
    pub source_drops: AtomicUsize,
    pub event_drops: AtomicUsize,
    pub event_dispatches: AtomicUsize,
    pub setup_future_drops: AtomicUsize,
    pub event_future_drops: AtomicUsize,
    pub outer_maps: AtomicUsize,
    pub source_maps: AtomicUsize,
    pub event_maps: AtomicUsize,
}

pub struct State {
    pub counts: Counts,
    pub setup_mode: SetupMode,
    pub event_mode: EventMode,
    pub(super) source: Mutex<Vec<SourceStep>>,
    pub setup_generations: Mutex<Vec<usize>>,
    pub event_generations: Mutex<Vec<usize>>,
    pub retained_arguments: Mutex<Vec<i32>>,
}

impl State {
    pub fn new(source: Vec<SourceStep>) -> Arc<Self> {
        Arc::new(Self {
            counts: Counts::default(),
            setup_mode: SetupMode::Ready,
            event_mode: EventMode::Object,
            source: Mutex::new(source),
            setup_generations: Mutex::new(Vec::new()),
            event_generations: Mutex::new(Vec::new()),
            retained_arguments: Mutex::new(Vec::new()),
        })
    }

    pub fn modes(source: Vec<SourceStep>, setup: SetupMode, event: EventMode) -> Arc<Self> {
        let mut state = Self::new(source);
        let value = Arc::get_mut(&mut state).unwrap();
        value.setup_mode = setup;
        value.event_mode = event;
        state
    }
}

pub struct AppContext {
    pub generation: usize,
    drops: Arc<AtomicUsize>,
}

impl AppContext {
    pub fn new(generation: usize) -> Self {
        Self::with_shared_drops(generation, Arc::new(AtomicUsize::new(0)))
    }

    pub fn with_shared_drops(generation: usize, drops: Arc<AtomicUsize>) -> Self {
        Self { generation, drops }
    }
}

impl Drop for AppContext {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn count(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}
