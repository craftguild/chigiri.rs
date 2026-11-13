//! Bounded exporter queues that never block request tasks.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};
use crossbeam_channel::{
    Receiver, Sender, TryRecvError as ChannelTryRecvError, TrySendError, bounded,
};

/// Result of a non-blocking export attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TryExport {
    Exported,
    DroppedFull,
    Disconnected,
}

/// Producer handle for a bounded multi-producer queue.
pub struct BoundedExporterQueue<T> {
    sender: Sender<T>,
    dropped: Arc<AtomicU64>,
}

impl<T> Clone for BoundedExporterQueue<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            dropped: Arc::clone(&self.dropped),
        }
    }
}

impl<T> BoundedExporterQueue<T> {
    /// Creates a producer/consumer pair with nonzero capacity.
    pub fn new(capacity: usize) -> Result<(Self, ExporterReceiver<T>), QueueBuildError> {
        if capacity == 0 {
            return Err(QueueBuildError);
        }
        let (sender, receiver) = bounded(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        Ok((
            Self {
                sender,
                dropped: Arc::clone(&dropped),
            },
            ExporterReceiver { receiver, dropped },
        ))
    }

    /// Attempts export without blocking the request task; increments the
    /// dropped-event count when the queue is full or disconnected.
    pub fn try_export(&self, value: T) -> TryExport {
        match self.sender.try_send(value) {
            Ok(()) => TryExport::Exported,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                TryExport::DroppedFull
            }
            Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                TryExport::Disconnected
            }
        }
    }

    /// Returns the cumulative number of items dropped because the queue was
    /// full or disconnected.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The consumer handle is owned by the exporter worker.
pub struct ExporterReceiver<T> {
    receiver: Receiver<T>,
    dropped: Arc<AtomicU64>,
}

impl<T> ExporterReceiver<T> {
    /// Attempts to receive one item without blocking, for tests or the worker loop.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.receiver.try_recv().map_err(|error| match error {
            ChannelTryRecvError::Empty => TryRecvError::Empty,
            ChannelTryRecvError::Disconnected => TryRecvError::Disconnected,
        })
    }
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// A queue was configured with zero capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueBuildError;

impl fmt::Display for QueueBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("exporter queue capacity must be greater than zero")
    }
}

impl std::error::Error for QueueBuildError {}

/// Owned lifecycle record without secret or high-cardinality fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleRecord {
    pub phase: LifecyclePhase,
    pub result: LifecycleResult,
    pub duration: Option<Duration>,
    pub code: Option<&'static str>,
    pub component: &'static str,
    pub has_request_id: bool,
    pub has_operation: bool,
}

impl LifecycleRecord {
    fn from_event(event: LifecycleEvent<'_>) -> Self {
        let identity = event.identity();
        Self {
            phase: event.phase(),
            result: event.result(),
            duration: event.duration(),
            code: event.code(),
            component: event.component(),
            has_request_id: identity.request_id().is_some(),
            has_operation: identity.operation().is_some(),
        }
    }
}

/// Owned error record without internal source, path, or identity values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorRecord {
    pub phase: LifecyclePhase,
    pub code: &'static str,
    pub component: &'static str,
    pub has_error_id: bool,
    pub has_request_id: bool,
    pub has_operation: bool,
    pub has_path: bool,
    pub has_coordinate: bool,
}

impl ErrorRecord {
    fn from_event(event: ErrorEvent<'_>) -> Self {
        let identity = event.identity();
        Self {
            phase: event.phase(),
            code: event.code(),
            component: event.component(),
            has_error_id: event.error_id().is_some(),
            has_request_id: identity.request_id().is_some(),
            has_operation: identity.operation().is_some(),
            has_path: event.path().is_some(),
            has_coordinate: event.coordinate().is_some(),
        }
    }
}

/// Bridge connecting lifecycle/error observer to two independent bounded queues.
#[derive(Clone)]
pub struct LifecycleExporter {
    lifecycle: BoundedExporterQueue<LifecycleRecord>,
    errors: BoundedExporterQueue<ErrorRecord>,
}

impl LifecycleExporter {
    /// Creates lifecycle and error queues with the same capacity.
    pub fn new(
        capacity: usize,
    ) -> Result<
        (
            Self,
            ExporterReceiver<LifecycleRecord>,
            ExporterReceiver<ErrorRecord>,
        ),
        QueueBuildError,
    > {
        let (lifecycle, lifecycle_receiver) = BoundedExporterQueue::new(capacity)?;
        let (errors, error_receiver) = BoundedExporterQueue::new(capacity)?;
        Ok((
            Self { lifecycle, errors },
            lifecycle_receiver,
            error_receiver,
        ))
    }

    pub fn dropped_lifecycle(&self) -> u64 {
        self.lifecycle.dropped_count()
    }
    pub fn dropped_errors(&self) -> u64 {
        self.errors.dropped_count()
    }
}

impl LifecycleObserver for LifecycleExporter {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.lifecycle
            .try_export(LifecycleRecord::from_event(event));
    }
}

impl ErrorObserver for LifecycleExporter {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.errors.try_export(ErrorRecord::from_event(event));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::TryRecvError;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::{BoundedExporterQueue, TryExport};

    #[test]
    fn full_queue_does_not_block_and_increments_the_drop_count() {
        let (queue, receiver) = BoundedExporterQueue::new(1).unwrap();
        assert_eq!(queue.try_export(1), TryExport::Exported);
        assert_eq!(queue.try_export(2), TryExport::DroppedFull);
        assert_eq!(receiver.dropped_count(), 1);
        assert_eq!(receiver.try_recv(), Ok(1));
    }

    #[test]
    fn queue_preserves_fifo_order_and_bidirectional_disconnects() {
        let (queue, receiver) = BoundedExporterQueue::new(2).unwrap();
        assert_eq!(queue.try_export(1), TryExport::Exported);
        assert_eq!(queue.try_export(2), TryExport::Exported);
        assert_eq!(receiver.try_recv(), Ok(1));
        assert_eq!(receiver.try_recv(), Ok(2));
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        drop(queue);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Disconnected));

        let (queue, receiver) = BoundedExporterQueue::<u8>::new(1).unwrap();
        let cloned = queue.clone();
        drop(queue);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
        drop(cloned);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Disconnected));

        let (queue, receiver) = BoundedExporterQueue::new(1).unwrap();
        drop(receiver);
        assert_eq!(queue.try_export(3), TryExport::Disconnected);
        assert_eq!(queue.dropped_count(), 1);
    }

    #[test]
    fn parallel_producers_deliver_to_capacity_with_consistent_counts_and_fifo_order() {
        const PRODUCERS: usize = 8;
        const ATTEMPTS_PER_PRODUCER: usize = 1_024;
        const CAPACITY: usize = 4_096;

        let (queue, receiver) = BoundedExporterQueue::new(CAPACITY).unwrap();
        let barrier = Arc::new(Barrier::new(PRODUCERS));
        let handles = (0..PRODUCERS)
            .map(|producer| {
                let queue = queue.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    let mut counts = [0usize; 3];
                    for sequence in 0..ATTEMPTS_PER_PRODUCER {
                        match queue.try_export((producer, sequence)) {
                            TryExport::Exported => counts[0] += 1,
                            TryExport::DroppedFull => counts[1] += 1,
                            TryExport::Disconnected => counts[2] += 1,
                        }
                    }
                    counts
                })
            })
            .collect::<Vec<_>>();
        drop(queue);

        let mut counts = [0usize; 3];
        for handle in handles {
            let producer_counts = handle.join().unwrap();
            for index in 0..counts.len() {
                counts[index] += producer_counts[index];
            }
        }
        let attempts = PRODUCERS * ATTEMPTS_PER_PRODUCER;
        assert_eq!(attempts, counts.iter().sum());
        assert_eq!(counts, [CAPACITY, attempts - CAPACITY, 0]);
        assert_eq!(
            receiver.dropped_count(),
            u64::try_from(counts[1] + counts[2]).unwrap()
        );

        let mut sequences = vec![Vec::new(); PRODUCERS];
        loop {
            match receiver.try_recv() {
                Ok((producer, sequence)) => sequences[producer].push(sequence),
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => panic!("After producer, queue became empty."),
            }
        }
        assert_eq!(sequences.iter().map(Vec::len).sum::<usize>(), counts[0]);
        for values in sequences {
            assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }
}
