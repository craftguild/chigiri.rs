//! Fairly move one staged transition at a time without running user code.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod planning;
use planning::{commit_cursor, plan_binding, reclaim_or_pressure};

use super::dispatcher::IncrementalDispatcher;
use super::retained::{CapturedEvent, RetainedBatch};
use super::source::{Offer, SourcePort, Termination};
use super::staging::StagedBatch;
use super::topics::RetiredBinding;
use super::{PublicationError, PublicationStore};

pub(crate) struct PublicationPump {
    dispatcher: IncrementalDispatcher,
}

enum Plan {
    Reclaim(StagedBatch),
    Pressure(Arc<SourcePort>),
    Tick {
        port: Arc<SourcePort>,
        event: CapturedEvent,
        complete: bool,
    },
    Retire {
        port: Option<Arc<SourcePort>>,
        reason: Termination,
        retired: RetiredBinding,
    },
    Offer {
        port: Arc<SourcePort>,
        generation: u64,
        expected_sequence: u64,
        expected_publication: usize,
        next_sequence: u64,
        next_publication: usize,
        event: CapturedEvent,
    },
    Advanced,
    Scanned(Arc<SourcePort>),
    Idle,
}

impl PublicationPump {
    pub(crate) const fn new() -> Self {
        Self {
            dispatcher: IncrementalDispatcher::new(),
        }
    }

    /// Returns true only while bounded work remains immediately runnable.
    pub(crate) fn step(
        &mut self,
        store: &Arc<Mutex<PublicationStore>>,
    ) -> Result<bool, PublicationError> {
        self.step_at(store, Instant::now())
    }

    pub(in crate::publication) fn step_at(
        &mut self,
        store: &Arc<Mutex<PublicationStore>>,
        now: Instant,
    ) -> Result<bool, PublicationError> {
        let plan = {
            let mut store = store.lock().map_err(|_| PublicationError::Invariant)?;
            self.plan(&mut store, now)?
        };
        match plan {
            Plan::Tick {
                port,
                event,
                complete,
            } => {
                match port.offer_at(event, now) {
                    Offer::Accepted => {
                        if complete {
                            port.finish()?;
                        }
                    }
                    Offer::Failed(event) => {
                        drop(event);
                        return Err(PublicationError::Invariant);
                    }
                }
                Ok(true)
            }
            Plan::Pressure(port) => {
                port.relieve_staging_pressure()?;
                Ok(true)
            }
            Plan::Reclaim(batch) => {
                drop(batch);
                Ok(true)
            }
            Plan::Retire {
                port,
                reason,
                retired,
            } => {
                drop(retired);
                if let Some(port) = port {
                    port.terminate(reason)?;
                }
                Ok(true)
            }
            Plan::Offer {
                port,
                generation,
                expected_sequence,
                expected_publication,
                next_sequence,
                next_publication,
                event,
            } => match port.offer_at(event, now) {
                Offer::Failed(event) => {
                    drop(event);
                    Err(PublicationError::Invariant)
                }
                Offer::Accepted => {
                    let committed = {
                        let mut store = store.lock().map_err(|_| PublicationError::Invariant)?;
                        commit_cursor(
                            &mut store,
                            generation,
                            expected_sequence,
                            expected_publication,
                            next_sequence,
                            next_publication,
                        )
                    };
                    if committed {
                        Ok(true)
                    } else {
                        port.terminate(Termination::Invariant)?;
                        Err(PublicationError::Invariant)
                    }
                }
            },
            Plan::Advanced => Ok(true),
            Plan::Scanned(port) => {
                port.expire(now)?;
                Ok(self.dispatcher.runnable())
            }
            Plan::Idle => Ok(false),
        }
    }

    pub(crate) fn shutdown_step(
        &mut self,
        store: &Arc<Mutex<PublicationStore>>,
    ) -> Result<bool, PublicationError> {
        let retired = {
            let mut store = store.lock().map_err(|_| PublicationError::Invariant)?;
            let Some(binding) = store.topics.bindings.first() else {
                return Ok(false);
            };
            let port = binding.port.upgrade();
            let retired = store.topics.retire_index(0);
            (port, retired)
        };
        drop(retired.1);
        if let Some(port) = retired.0 {
            port.terminate(Termination::Shutdown)?;
        }
        Ok(true)
    }

    pub(crate) fn wait_duration(
        &self,
        store: &Arc<Mutex<PublicationStore>>,
        now: Instant,
        maximum: Duration,
    ) -> Result<Duration, PublicationError> {
        let store = store.lock().map_err(|_| PublicationError::Invariant)?;
        Ok(store
            .topics
            .bindings
            .iter()
            .filter_map(|binding| binding.sequence.as_ref()?.deadline())
            .map(|deadline| deadline.saturating_duration_since(now))
            .min()
            .unwrap_or(maximum)
            .min(maximum))
    }

    fn plan(
        &mut self,
        store: &mut PublicationStore,
        now: Instant,
    ) -> Result<Plan, PublicationError> {
        if let Some(plan) = reclaim_or_pressure(store)? {
            return Ok(plan);
        }
        let Some(index) = self.dispatcher.select_binding(&store.topics) else {
            return Ok(Plan::Idle);
        };
        plan_binding(store, index, now)
    }
}
