//! Validate startup coverage and install immutable host settings before activation.

use std::sync::Arc;

use super::{Worker, environment};
use crate::luau::{Contract, HookScript, ScriptCandidate, ScriptError, ScriptSet};
use crate::store::Store;

pub(super) struct LaunchSettings {
    pub source_limits: crate::events::SourceLimits,
    pub clock: crate::clock::ClockSettings,
    pub consumers: Vec<crate::luau::ConsumerScript>,
    pub logger: Option<crate::logging::Logger>,
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            source_limits: Default::default(),
            clock: crate::clock::ClockSettings::system(),
            consumers: Vec::new(),
            logger: None,
        }
    }
}

impl Worker {
    /// Validate all startup scripts on a dedicated stack, then launch and return the worker client.
    pub fn start(
        contract: Contract,
        candidates: Vec<ScriptCandidate>,
        store: Store,
    ) -> Result<Self, ScriptError> {
        if !Arc::ptr_eq(contract.ir(), store.snapshot().ir()) {
            return Err(ScriptError::new(
                "contract",
                "worker schema and store differ",
            ));
        }
        Self::launch(
            contract.ir().clone(),
            contract.state().clone(),
            Vec::new(),
            store,
            LaunchSettings::default(),
            move || ScriptSet::new(contract, candidates),
        )
    }

    /// Start from the scripts and store produced by validation without reinitializing Luau.
    pub fn from_checked(
        scripts: ScriptSet,
        hooks: Vec<HookScript>,
        store: Store,
    ) -> Result<Self, ScriptError> {
        let ir = scripts.contract().ir().clone();
        if !Arc::ptr_eq(&ir, store.snapshot().ir()) {
            return Err(ScriptError::new(
                "contract",
                "worker schema and store differ",
            ));
        }
        Self::launch(
            ir,
            scripts.contract().state().clone(),
            hooks,
            store,
            LaunchSettings::default(),
            move || Ok(scripts),
        )
    }

    pub fn from_checked_with_clock(
        scripts: ScriptSet,
        hooks: Vec<HookScript>,
        store: Store,
        clock: crate::clock::ClockSettings,
    ) -> Result<Self, ScriptError> {
        let ir = scripts.contract().ir().clone();
        if !Arc::ptr_eq(&ir, store.snapshot().ir()) {
            return Err(environment("worker schema and store differ"));
        }
        Self::launch(
            ir,
            scripts.contract().state().clone(),
            hooks,
            store,
            LaunchSettings {
                clock,
                ..LaunchSettings::default()
            },
            move || Ok(scripts),
        )
    }

    /// Start with the complete checked event policy before opening a listener.
    pub fn from_checked_with_events(
        scripts: ScriptSet,
        hooks: Vec<HookScript>,
        store: Store,
        clock: crate::clock::ClockSettings,
        events: crate::events::EventSettings,
        consumers: Vec<crate::luau::ConsumerScript>,
    ) -> Result<Self, ScriptError> {
        Self::from_checked_with_events_logged(scripts, hooks, store, clock, events, consumers, None)
    }

    /// Install logging and apparent time before any named setup or callback.
    pub fn from_checked_with_events_logged(
        scripts: ScriptSet,
        hooks: Vec<HookScript>,
        store: Store,
        clock: crate::clock::ClockSettings,
        events: crate::events::EventSettings,
        consumers: Vec<crate::luau::ConsumerScript>,
        logger: Option<crate::logging::Logger>,
    ) -> Result<Self, ScriptError> {
        let expected = events
            .resolve(scripts.contract().ir())
            .map_err(|_| environment("consumer registration field mismatch"))?;
        let names: std::collections::BTreeSet<_> = consumers
            .iter()
            .map(|consumer| consumer.registration().name())
            .collect();
        if expected.len() != consumers.len()
            || names.len() != consumers.len()
            || consumers.iter().any(|consumer| {
                !Arc::ptr_eq(consumer.script().ir(), scripts.contract().ir())
                    || !expected
                        .iter()
                        .any(|entry| entry == consumer.registration())
            })
        {
            return Err(environment("consumer registration coverage mismatch"));
        }
        let ir = scripts.contract().ir().clone();
        if !Arc::ptr_eq(&ir, store.snapshot().ir()) {
            return Err(environment("worker schema and store differ"));
        }
        Self::launch(
            ir,
            scripts.contract().state().clone(),
            hooks,
            store,
            LaunchSettings {
                source_limits: events.limits(),
                clock,
                consumers,
                logger,
            },
            move || Ok(scripts),
        )
    }
}
