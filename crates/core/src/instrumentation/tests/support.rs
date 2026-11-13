//! Thread-local test backend with observable context and completion ownership.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use crate::instrumentation::*;
use crate::{ErrorEvent, LifecycleResult, SamplingDecision};

thread_local! {
    pub(super) static CURRENT: Cell<(u8, bool)> = const { Cell::new((0, false)) };
}

pub(super) struct Guard((u8, bool));

impl Guard {
    pub(super) fn install(value: (u8, bool)) -> Self {
        Self(CURRENT.replace(value))
    }
}

impl InstrumentationGuard for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.set(self.0);
    }
}

pub(super) type Ends = Arc<Mutex<Vec<(LifecycleResult, (u8, bool))>>>;

pub(super) struct Backend {
    id: u8,
    suppressed: bool,
    ends: Ends,
}

pub(super) fn scope(value: (u8, bool)) -> (InstrumentationScope, Ends) {
    let ends = Arc::default();
    let scope = InstrumentationScope::new(Backend {
        id: value.0,
        suppressed: value.1,
        ends: Arc::clone(&ends),
    });
    (scope, ends)
}

impl ExecutionScope for Backend {
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard::install((self.id, self.suppressed)))
    }

    fn context(&self) -> PropagationContext {
        PropagationContext::new(None, self.suppressed)
    }

    fn child(&self, _: InstrumentationKind, _: SamplingDecision) -> InstrumentationScope {
        let (id, suppressed) = CURRENT.get();
        scope((id + 1, suppressed)).0
    }

    fn observe_error(&self, _: &ErrorEvent<'_>) {}

    fn finish(&self, result: LifecycleResult) {
        self.ends.lock().unwrap().push((result, CURRENT.get()));
    }
}
