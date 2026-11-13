//! Own registered sources on the existing VM scheduler, with no retained VM.

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::cancellation::Cancellation;
use crate::logging::{ConsumerOutcome, Logger};
use crate::luau::{ConsumerScript, ScriptError};
use crate::publication::{PublicationStore, RegisteredSubscription};
use crate::runtime::{self, Execution, ExecutionBudget, HookLogSink};

mod execution;
mod startup;

#[derive(Clone)]
pub(super) struct ConsumerHost {
    pub clock: Arc<crate::clock::ServerClock>,
    pub mutation: Arc<tokio::sync::Mutex<()>>,
    pub logger: Option<Logger>,
    pub events: Arc<super::event_lifecycle::EventLifecycle>,
}

struct Entry {
    script: ConsumerScript,
    source: Option<RegisteredSubscription>,
    invocation: u64,
    log: HookLogSink,
}

pub(super) struct ConsumerSet {
    entries: Vec<Entry>,
    next: usize,
    host: ConsumerHost,
    waker: Waker,
    pub wake: Receiver<()>,
}

struct Ready(Sender<()>);
impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        let _ = self.0.try_send(());
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.try_send(());
    }
}

impl ConsumerSet {
    pub(super) fn prepare(
        store: &Arc<Mutex<PublicationStore>>,
        consumers: Vec<ConsumerScript>,
        host: ConsumerHost,
        shutdown: &Cancellation,
    ) -> Result<Self, ScriptError> {
        let (ready, wake) = bounded(1);
        host.events.install(ready.clone());
        let mut set = Self {
            entries: Vec::new(),
            next: 0,
            host,
            waker: Waker::from(Arc::new(Ready(ready))),
            wake,
        };
        for consumer in consumers {
            if consumer.registration().enabled() {
                set.entries
                    .push(startup::prepare(store, consumer, &set.host, shutdown)?);
            }
        }
        // No callback or HTTP job can run before the full set has registered.
        // Establish every sequence origin only after the final startup win.
        for entry in &set.entries {
            entry
                .source
                .as_ref()
                .expect("prepared consumer source")
                .activate();
        }
        Ok(set)
    }

    /// Execute at most one ready input, fairly interleaved with ordinary jobs.
    /// The permit is nonblocking: a GraphQL request may need this same VM to
    /// finish the work that releases its write admission.
    pub(super) fn step(
        &mut self,
        store: &Arc<Mutex<PublicationStore>>,
        shutdown: &Cancellation,
    ) -> bool {
        if self.host.events.stopped() {
            self.stop();
            return false;
        }
        if self.entries.is_empty() || shutdown.cancelled() {
            return false;
        }
        let Ok(_permit) = self.host.mutation.clone().try_lock_owned() else {
            return false;
        };
        let lifecycle = self.host.events.clone();
        let Some(start) = lifecycle.start() else {
            return false;
        };
        for _ in 0..self.entries.len() {
            let index = self.next;
            self.next = (self.next + 1) % self.entries.len();
            let entry = &mut self.entries[index];
            let Some(source) = &mut entry.source else {
                continue;
            };
            // Poll is the callback-start boundary. It samples actual source
            // evictions only after admission, never while queued behind a job.
            match source.poll(&mut Context::from_waker(&self.waker)) {
                Poll::Pending => {}
                Poll::Ready(Some(Ok(event))) => {
                    if entry.invocation == crate::publication::MAX_SAFE_SEQUENCE {
                        drop(start);
                        stop(entry, &self.host.logger, "counter");
                        return true;
                    }
                    let _running = lifecycle.running(start);
                    entry.invocation += 1;
                    let result = execution::run(store, entry, &event, &self.host, shutdown);
                    if let Some(logger) = &self.host.logger {
                        logger.consumer_terminal(
                            entry.script.registration().name(),
                            entry.script.script().coordinate(),
                            entry.invocation,
                            if result.is_ok() {
                                ConsumerOutcome::Committed
                            } else {
                                ConsumerOutcome::RolledBack
                            },
                            result.as_ref().err().map_or("success", |error| error.cause),
                        );
                    }
                    // Keep the lease and write permit through actual VM exit,
                    // final validation and publication transfer; do not retry.
                    drop(event);
                    return true;
                }
                Poll::Ready(Some(Err(_))) => {
                    drop(start);
                    stop(entry, &self.host.logger, "source");
                    return true;
                }
                Poll::Ready(None) => {
                    drop(start);
                    stop(entry, &self.host.logger, "completed");
                    return true;
                }
            }
        }
        false
    }

    fn stop(&mut self) {
        for entry in &mut self.entries {
            if entry.source.is_some() {
                stop(entry, &self.host.logger, "shutdown");
            }
        }
    }
}

fn stop(entry: &mut Entry, logger: &Option<Logger>, cause: &'static str) {
    drop(entry.source.take());
    if let Some(logger) = logger {
        logger.consumer_terminal(
            entry.script.registration().name(),
            entry.script.script().coordinate(),
            entry.invocation,
            ConsumerOutcome::Stopped,
            cause,
        );
    }
}

impl Drop for ConsumerSet {
    fn drop(&mut self) {
        self.stop();
    }
}

fn execution(host: &ConsumerHost, script: &ConsumerScript, shutdown: &Cancellation) -> Execution {
    let now = Instant::now();
    let budget = ExecutionBudget::default();
    budget.0.start_request(now);
    budget.set_clock(&host.clock);
    Execution {
        state: script.script().initial_state(),
        request: None,
        deadline: now + runtime::VM_TIME,
        cancel: Cancellation::default(),
        shutdown: shutdown.clone(),
        budget,
    }
}

fn log(host: &ConsumerHost, script: &ConsumerScript, phase: &'static str) -> HookLogSink {
    let logger = host.logger.clone();
    let name = script.registration().name().to_owned();
    let coordinate = script.script().coordinate().to_owned();
    Arc::new(move |record| {
        if let Some(logger) = &logger {
            logger.consumer_log(&name, &coordinate, phase, &record);
        }
    })
}

#[cfg(test)]
mod tests;
