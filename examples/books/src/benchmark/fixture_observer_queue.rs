//! Observer-queue draining and delivery markers for benchmark samples.

use std::sync::mpsc::TryRecvError;

use chigiri_observability::ExporterReceiver;

use super::{Fixture, LIFECYCLE_EVENTS_PER_REQUEST, OBSERVER_CAPACITY};
use crate::benchmark::model::ObserverQueueEvidence;

impl Fixture {
    /// Remove build-time events before measurement and verify queue integrity.
    pub(in crate::benchmark) fn prepare_observer_measurement(
        &self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(counts) = self.drain_observers()? else {
            return Ok(());
        };
        if counts.lifecycle != 1 || counts.errors != 0 || self.dropped_count() != (0, 0) {
            return Err(format!(
                "{} observer queue before measurement does not contain only one ServerBuilt event",
                self.profile_name
            )
            .into());
        }
        Ok(())
    }

    /// Validate events outside the timed sample, then drain the expected count.
    pub(in crate::benchmark) fn drain_observer_sample(
        &self,
        iterations: usize,
    ) -> Result<Option<ObserverSampleCounts>, Box<dyn std::error::Error>> {
        let Some(counts) = self.drain_observers()? else {
            return Ok(None);
        };
        let expected = iterations
            .checked_mul(LIFECYCLE_EVENTS_PER_REQUEST)
            .ok_or("The expected observer event count overflowed.")?;
        if counts.lifecycle != expected || counts.errors != 0 || self.dropped_count() != (0, 0) {
            return Err(format!(
                "{} observer queue delivery count violates the contract: expected={expected}, lifecycle={}, errors={}, dropped={:?}",
                self.profile_name, counts.lifecycle, counts.errors, self.dropped_count()
            )
            .into());
        }
        Ok(Some(counts))
    }

    pub(in crate::benchmark) fn observer_queue_evidence(
        &self,
        samples: usize,
        iterations: usize,
        counts: ObserverSampleCounts,
    ) -> Result<Option<ObserverQueueEvidence>, Box<dyn std::error::Error>> {
        if self.lifecycle_receiver.is_none() {
            return Ok(None);
        }
        let expected = samples
            .checked_mul(iterations)
            .and_then(|value| value.checked_mul(LIFECYCLE_EVENTS_PER_REQUEST))
            .ok_or("observer evidence expectation count overflowed")?;
        let (dropped_lifecycle, dropped_errors) = self.dropped_count();
        Ok(Some(ObserverQueueEvidence {
            name: self.profile_name.to_owned(),
            capacity: OBSERVER_CAPACITY,
            lifecycle_events_per_request: LIFECYCLE_EVENTS_PER_REQUEST,
            expected_lifecycle_event_count: u64::try_from(expected)?,
            accepted_lifecycle_event_count: u64::try_from(counts.lifecycle)?,
            dropped_lifecycle_event_count: dropped_lifecycle,
            accepted_error_event_count: u64::try_from(counts.errors)?,
            dropped_error_event_count: dropped_errors,
        }))
    }

    fn drain_observers(&self) -> Result<Option<ObserverSampleCounts>, Box<dyn std::error::Error>> {
        let Some(lifecycle) = &self.lifecycle_receiver else {
            return Ok(None);
        };
        let errors = self
            .error_receiver
            .as_ref()
            .ok_or("observer error receiver is missing")?;
        Ok(Some(ObserverSampleCounts {
            lifecycle: drain(lifecycle)?,
            errors: drain(errors)?,
        }))
    }

    fn dropped_count(&self) -> (u64, u64) {
        (
            self.lifecycle_receiver
                .as_ref()
                .map_or(0, ExporterReceiver::dropped_count),
            self.error_receiver
                .as_ref()
                .map_or(0, ExporterReceiver::dropped_count),
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::benchmark) struct ObserverSampleCounts {
    pub(super) lifecycle: usize,
    pub(super) errors: usize,
}

impl ObserverSampleCounts {
    pub(in crate::benchmark) fn add(&mut self, other: Self) {
        self.lifecycle += other.lifecycle;
        self.errors += other.errors;
    }
}

fn drain<T>(receiver: &ExporterReceiver<T>) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    loop {
        match receiver.try_recv() {
            Ok(_) => count += 1,
            Err(TryRecvError::Empty) => return Ok(count),
            Err(TryRecvError::Disconnected) => {
                return Err("The observer queue is cut.".into());
            }
        }
    }
}
