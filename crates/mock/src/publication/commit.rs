//! Linearize topic ordering and the store exchange without fan-out or VM work.

use std::sync::Arc;

use super::retained::RetainedBatch;
use super::staging::{DispatchStaging, RetainedStageFailure};
use super::{PreparedPublications, PublicationError, PublicationOwner, TopicIndex};
use crate::store::{RequestCandidate, RequestPrepared, Snapshot, Store, StoreError};
use crate::value::{ValueBudget, ValueError};

/// The worker's single committed-state mutex also protects registration ordering.
/// Its immutable Store IR owns schema identity once for this server. Captures do
/// not duplicate that digest: opaque staging/sequence tokens keep batches within
/// this owner, while each Record digest remains bound to the schema and old fields.
pub(crate) struct PublicationStore {
    store: Store,
    pub(super) topics: TopicIndex,
    pub(super) staging: Arc<DispatchStaging>,
    source_limits: crate::events::SourceLimits,
    retention: crate::events::retention::Retention,
    wake: Arc<super::wake::DeliveryWake>,
}

/// Retired ownership leaves both the store lock and cancellation fence intact.
pub(crate) struct CommitRetired {
    _snapshot: Snapshot,
    _failed_transfer: Option<RetainedStageFailure>,
    observation: Option<CommittedPublications>,
}

struct CommittedPublications {
    retained: Arc<RetainedBatch>,
    sequence: u64,
    lost: bool,
}

/// A borrowed safe staging decision, constructed only from a committed batch.
pub(crate) struct CommittedPublication<'a> {
    pub(crate) sequence: u64,
    pub(crate) index: usize,
    pub(crate) coordinate: &'a str,
    pub(crate) lost: bool,
}

impl CommitRetired {
    /// Borrow only bounded schema/order metadata after leaving the commit fence.
    pub(crate) fn observations(&self) -> impl Iterator<Item = CommittedPublication<'_>> {
        self.observation.iter().flat_map(|batch| {
            batch
                .retained
                .batch()
                .publications()
                .iter()
                .enumerate()
                .map(move |(index, publication)| CommittedPublication {
                    sequence: batch.sequence,
                    index,
                    coordinate: publication.coordinate(),
                    lost: batch.lost,
                })
        })
    }
}

impl PublicationStore {
    #[cfg(test)]
    pub(crate) fn new(store: Store, max_bindings: usize) -> Result<Self, PublicationError> {
        Self::with_source_limits(store, max_bindings, crate::events::SourceLimits::default())
    }

    pub(crate) fn with_source_limits(
        store: Store,
        max_bindings: usize,
        source_limits: crate::events::SourceLimits,
    ) -> Result<Self, PublicationError> {
        let topics = TopicIndex::new(max_bindings)?;
        let staging = Arc::new(DispatchStaging::new(&topics)?);
        Ok(Self {
            store,
            topics,
            staging,
            source_limits,
            retention: crate::events::retention::Retention::new(source_limits.max_retained_bytes()),
            wake: Arc::default(),
        })
    }

    pub(crate) fn source_policy(
        &self,
    ) -> (
        crate::events::SourceLimits,
        crate::events::retention::Retention,
    ) {
        (self.source_limits, self.retention.clone())
    }

    pub(crate) fn delivery_wake(&self) -> Arc<super::wake::DeliveryWake> {
        self.wake.clone()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.store.snapshot()
    }

    pub(crate) fn begin_request(&self, values: ValueBudget) -> RequestCandidate {
        self.store.begin_request(values)
    }

    pub(crate) fn mutation_admission(&self) -> Result<(), PublicationError> {
        self.topics.check_admission()
    }

    /// Capture only fixed server metadata while locked, then allocate the request
    /// buffer after releasing the topic/store linearization lock.
    pub(crate) fn capture_owner(&self) -> PublicationOwner {
        PublicationOwner::new(
            self.store.snapshot().ir().clone(),
            self.staging.capture_owner(),
            self.retention.clone(),
        )
    }

    /// Clone only the bounded queue owner; capture/reservation happen after unlocking.
    pub(crate) fn staging(&self) -> Arc<DispatchStaging> {
        Arc::clone(&self.staging)
    }

    /// Validate the opaque staging owner without exposing it to the projection path.
    pub(crate) fn owns_event(&self, event: &super::CapturedEvent) -> bool {
        event.belongs_to(&self.staging, &self.retention)
    }

    /// Retained finite jobs have no publication capability, but share commit order.
    pub(crate) fn commit_request(
        &mut self,
        prepared: &RequestPrepared,
    ) -> Result<CommitRetired, StoreError> {
        self.commit_publications(prepared, &mut None)
    }

    /// Called with the topic/store linearization lock and cancellation fence held.
    /// A transfer failure after the store win is delivery loss, not rollback.
    pub(crate) fn commit_publications(
        &mut self,
        prepared: &RequestPrepared,
        publications: &mut Option<PreparedPublications>,
    ) -> Result<CommitRetired, StoreError> {
        self.store.check_request(prepared)?;
        if let Some(publications) = publications.as_ref() {
            self.staging
                .validate_capture(
                    publications.retained.reservation(),
                    publications.retained.batch(),
                )
                .map_err(|_| ValueError("foreign or invalid publication capture"))?;
        }
        let sequence = self
            .topics
            .commit_sequence()
            .map_err(|_| ValueError("publication ordering closed"))?;
        let snapshot = self.store.commit_request(prepared)?;
        // Sharing the original charge allocates no payload or metadata copy.
        // No sink, request, IR, or credentials enter the retained batch.
        let mut observation = publications.as_ref().map(|prepared| CommittedPublications {
            retained: Arc::clone(&prepared.retained),
            sequence: sequence.value(),
            lost: false,
        });
        let failed_transfer = publications.take().and_then(|prepared| {
            match self.staging.stage_retained(prepared.retained, sequence) {
                Ok(()) => None,
                Err(failed) => {
                    // Both values originated from this owner. Even an invariant failure
                    // must not let any eligible binding silently skip a committed batch.
                    if self
                        .topics
                        .raise_loss(&failed.sequence, failed.retained.batch())
                        .is_err()
                    {
                        // An impossible owner mismatch still closes later admission.
                        // Retain the retired metadata for disposal outside the fence.
                        self.topics.close_admission();
                    }
                    Some(failed)
                }
            }
        });
        if let Some(observation) = &mut observation {
            observation.lost = failed_transfer.is_some();
        }
        Ok(CommitRetired {
            _snapshot: snapshot,
            _failed_transfer: failed_transfer,
            observation,
        })
    }

    pub(crate) fn close(&mut self) -> super::topics::RetiredIndex {
        self.topics.shutdown()
    }
}
