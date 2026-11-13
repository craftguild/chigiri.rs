//! Deliver bounded JSONL through an isolated process without turning output loss into HTTP failure.

mod access;
mod consumer;
mod coordinator;
pub(crate) use consumer::ConsumerOutcome;
mod lifecycle;
mod pipe;
mod publication;
mod records;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub use crate::stdio_writer::{COMMAND, serve_writer};
pub use access::Access;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use serde::Serialize;

/// Logging handle exposed to host callbacks.
pub const FORMAT: &str = "chigiri-mock-log/v5";
/// Maximum UTF-8 byte length of one JSONL record; excess counts as delivery loss, not hook failure.
pub const RECORD_BYTES: usize = crate::stdio_writer::LINE_BYTES;
/// Queue capacity reserved independently for hook, access, and subscription records.
pub const QUEUE_RECORDS: usize = 64;
const NOTICE_RECORDS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Hook,
    Access,
    Subscription,
    Notice,
    Loss,
}
struct Record {
    kind: Kind,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Counters {
    hook: AtomicU64,
    access: AtomicU64,
    subscription: AtomicU64,
    notice: AtomicU64,
}

/// Cumulative delivery losses; loss-notification failure is not counted recursively.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Loss {
    /// Number of lost hook records.
    pub hook: u64,
    /// Number of lost access records.
    pub access: u64,
    /// Number of lost subscription decision records, not lost subscription events.
    pub subscription: u64,
    /// Number of lost diagnostic notices.
    pub notice: u64,
}

impl Counters {
    fn lose(&self, kind: Kind) {
        let counter = match kind {
            Kind::Hook => &self.hook,
            Kind::Access => &self.access,
            Kind::Subscription => &self.subscription,
            Kind::Notice => &self.notice,
            Kind::Loss => return,
        };
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            Some(count.saturating_add(1))
        });
    }
    fn snapshot(&self) -> Loss {
        Loss {
            hook: self.hook.load(Ordering::Relaxed),
            access: self.access.load(Ordering::Relaxed),
            subscription: self.subscription.load(Ordering::Relaxed),
            notice: self.notice.load(Ordering::Relaxed),
        }
    }
}

/// Producer handle that only serializes and attempts to enqueue; it never waits for stdio or the writer process.
#[derive(Clone)]
pub struct Logger {
    hook: Sender<Record>,
    access: Sender<Record>,
    subscription: Sender<Record>,
    notice: Sender<Record>,
    counts: Arc<Counters>,
    stopping: Arc<Mutex<Stop>>,
}

/// Closed producer retained by one subscription observation authority.
#[derive(Clone)]
pub(crate) struct SubscriptionSink {
    subscription: Sender<Record>,
    counts: Arc<Counters>,
    stopping: Arc<Mutex<Stop>>,
}

impl Logger {
    /// Deliver a hook log record or sanitized hook diagnostic with its location.
    pub fn hook(&self, event: crate::service::HookEvent) {
        let kind = if event.record.is_err() {
            Kind::Notice
        } else {
            Kind::Hook
        };
        self.enqueue(kind, records::hook(&event));
    }
    /// Deliver a resolver diagnostic through the existing stderr queue.
    pub fn resolver(&self, event: crate::service::ResolverDiagnostic) {
        self.enqueue(Kind::Notice, records::resolver(&event));
    }
    /// Enqueue a soft-limit warning without blocking request execution.
    pub fn limit_warning(&self, id: u64, notice: crate::limits::warning::LimitWarning) {
        self.enqueue(Kind::Notice, records::limit_warning(id, notice));
    }
    /// Send a terminal diagnostic containing the commit result to stderr.
    pub fn terminal(&self, event: crate::service::TerminalDiagnostic) {
        self.enqueue(Kind::Notice, records::terminal(&event));
    }
    /// Enqueue a human-readable notice encoded as one JSON line with escaped control characters.
    pub fn notice(&self, text: &str) {
        self.enqueue(Kind::Notice, records::notice(text));
    }
    /// Return cumulative delivery-loss counts.
    pub fn loss(&self) -> Loss {
        self.counts.snapshot()
    }
    pub(crate) fn publication(
        &self,
        id: u64,
        sequence: u64,
        index: usize,
        coordinate: &str,
        lost: bool,
    ) {
        self.enqueue(
            Kind::Subscription,
            publication::record(id, sequence, index, coordinate, lost),
        );
    }
    pub(crate) fn subscription_sink(&self) -> SubscriptionSink {
        SubscriptionSink {
            subscription: self.subscription.clone(),
            counts: Arc::clone(&self.counts),
            stopping: Arc::clone(&self.stopping),
        }
    }
    fn enqueue(&self, kind: Kind, bytes: Result<Vec<u8>, crate::value::ValueError>) {
        let queue = match kind {
            Kind::Hook => &self.hook,
            Kind::Access => &self.access,
            Kind::Subscription => &self.subscription,
            Kind::Notice => &self.notice,
            Kind::Loss => return,
        };
        let lost = bytes.map_or(true, |bytes| {
            // Serialize admission with shutdown so no record can enter after the final drain starts.
            let stopping = self.stopping.lock().expect("logging stop lock");
            stopping.close || queue.try_send(Record { kind, bytes }).is_err()
        });
        if lost {
            self.counts.lose(kind);
        }
    }
}

impl SubscriptionSink {
    pub(crate) fn setup_registered(
        &self,
        request_id: u64,
        coordinate: &str,
        subscription_id: u64,
        overflow: chigiri::SubscriptionOverflow,
    ) {
        self.enqueue(lifecycle::setup(
            request_id,
            coordinate,
            lifecycle::SetupOutcome::Registered {
                subscription_id,
                overflow,
            },
        ));
    }

    pub(crate) fn setup_unregistered(&self, request_id: u64, coordinate: &str, cancelled: bool) {
        self.enqueue(lifecycle::setup(
            request_id,
            coordinate,
            if cancelled {
                lifecycle::SetupOutcome::Cancelled
            } else {
                lifecycle::SetupOutcome::Failed
            },
        ));
    }

    pub(crate) fn drop_oldest(&self, generation: u64, coordinate: &str, total: u64) {
        self.enqueue(lifecycle::drop_oldest(generation, coordinate, total));
    }

    pub(crate) fn terminal(
        &self,
        generation: u64,
        coordinate: &str,
        decision: chigiri::__private::SubscriptionTermination,
        total: u64,
    ) {
        self.enqueue(lifecycle::terminal(generation, coordinate, decision, total));
    }

    fn enqueue(&self, bytes: Result<Vec<u8>, crate::value::ValueError>) {
        let lost = bytes.map_or(true, |bytes| {
            let stopping = self.stopping.lock().expect("logging stop lock");
            stopping.close
                || self
                    .subscription
                    .try_send(Record {
                        kind: Kind::Subscription,
                        bytes,
                    })
                    .is_err()
        });
        if lost {
            self.counts.lose(Kind::Subscription);
        }
    }
}

#[derive(Default)]
struct Stop {
    deadline: Option<Instant>,
    close: bool,
}

/// Owner of the logging thread and isolated stdio writer processes.
pub struct Writer {
    /// Producer handle used by HTTP and VM paths.
    pub logger: Logger,
    stopping: Arc<Mutex<Stop>>,
    thread: Option<JoinHandle<bool>>,
}

impl Writer {
    /// Start dedicated bounded workers for stdout and stderr.
    pub fn start(executable: &Path) -> Self {
        let (hook, hooks) = bounded(QUEUE_RECORDS);
        let (access, accesses) = bounded(QUEUE_RECORDS);
        let (subscription, subscriptions) = bounded(QUEUE_RECORDS);
        let (notice, notices) = bounded(NOTICE_RECORDS);
        let counts = Arc::new(Counters::default());
        let stopping = Arc::new(Mutex::new(Stop::default()));
        let logger = Logger {
            hook,
            access,
            subscription,
            notice,
            counts: counts.clone(),
            stopping: stopping.clone(),
        };
        let stop = stopping.clone();
        let executable = executable.to_owned();
        let thread = std::thread::Builder::new()
            .name("chigiri-mock-log".into())
            .spawn(move || {
                coordinator::run(
                    &executable,
                    [accesses, hooks, subscriptions],
                    notices,
                    counts,
                    stop,
                )
            })
            .ok();
        Self {
            logger,
            stopping,
            thread,
        }
    }
    /// Set one shutdown deadline shared by HTTP and VM paths without extending it later.
    pub fn deadline(&self, deadline: Instant) {
        let mut stop = self.stopping.lock().expect("logging stop lock");
        stop.deadline = Some(stop.deadline.map_or(deadline, |prior| prior.min(deadline)));
    }
    /// Return a callback that can only tighten the shared shutdown deadline.
    pub fn deadline_notifier(&self) -> impl Fn(Instant) + Send + Sync + 'static {
        let stopping = self.stopping.clone();
        move |deadline| {
            let mut stop = stopping.lock().expect("logging stop lock");
            stop.deadline = Some(stop.deadline.map_or(deadline, |prior| prior.min(deadline)));
        }
    }

    /// Drain accepted records within the deadline, then join the writer thread and child processes.
    pub async fn stop(&mut self, deadline: Instant) -> Result<Loss, &'static str> {
        self.deadline(deadline);
        self.stopping.lock().expect("logging stop lock").close = true;
        while self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            if Instant::now() >= deadline {
                return Err("log writer cleanup could not be confirmed");
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        if self
            .thread
            .take()
            .is_some_and(|thread| !matches!(thread.join(), Ok(true)))
        {
            return Err("log writer cleanup could not be confirmed");
        }
        Ok(self.logger.loss())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.deadline(Instant::now() + crate::limits::SHUTDOWN_TIME);
            self.stopping.lock().expect("logging stop lock").close = true;
            // The thread neither writes directly to stdio nor starts I/O beyond the established deadline.
            let _ = thread.join();
        }
    }
}
