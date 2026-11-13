//! Allocate binding metadata outside the store fence and linearize it once.

use std::sync::Arc;

use super::PublicationError;
use super::batch::{owned, owned_coordinate, validate_topic};
use super::source::SourcePort;
use super::topics::{BindingEntry, TopicEntry, TopicIndex};
use crate::ir::{FieldId, MockIr};

const BINDING_BOOKKEEPING_BYTES: usize = 64;

mod linearize;

pub(crate) struct RegistrationOwner {
    owner: Arc<()>,
    limits: crate::events::SourceLimits,
    retention: crate::events::retention::Retention,
    wake: Arc<super::wake::DeliveryWake>,
}

pub(crate) struct PreparedBinding {
    owner: Arc<()>,
    topic: Box<str>,
    topic_entry: Box<str>,
    coordinate: Box<str>,
    coordinate_entry: Box<str>,
    port: Arc<SourcePort>,
    sequence: Option<Box<super::timed::TimedSequence>>,
    retention: crate::events::retention::Reservation,
}

pub(super) struct RegisteredBinding {
    #[cfg(test)]
    owner: Arc<()>,
    generation: super::sequence::Generation,
    #[cfg(test)]
    registration: super::sequence::RegistrationSequence,
    port: Arc<SourcePort>,
}

pub(super) struct RegistrationOutcome {
    pub(super) binding: RegisteredBinding,
    pub(super) retired_topic_candidate: Option<(Box<str>, Box<str>)>,
}

pub(crate) struct RegistrationFailure {
    pub(super) error: PublicationError,
    pub(super) prepared: PreparedBinding,
}

pub(crate) struct RegistrationCommit {
    subscription: super::source::RegisteredSubscription,
    generation: super::sequence::Generation,
    retired_topic_candidate: Option<(Box<str>, Box<str>)>,
}

impl RegistrationOwner {
    pub(crate) fn prepare_sequence(
        &self,
        ir: &MockIr,
        field: FieldId,
        definition: &Arc<super::SequenceDefinition>,
    ) -> Result<PreparedBinding, PublicationError> {
        if !definition.belongs_to(&self.retention) || !definition.accepts(ir, field) {
            return Err(PublicationError::ForeignOwner);
        }
        if definition.bytes() > self.limits.max_sequence_bytes() {
            return Err(PublicationError::CaptureLimit);
        }
        let mut prepared = self.prepare_topic(ir, field, "")?;
        prepared
            .retention
            .grow(size_of::<super::timed::TimedSequence>())
            .map_err(|_| PublicationError::CaptureLimit)?;
        if !self.retention.fits(super::CapturedEvent::envelope_bytes()) {
            return Err(PublicationError::CaptureLimit);
        }
        prepared.sequence = Some(Box::new(super::timed::TimedSequence::new(
            definition.clone(),
        )));
        Ok(prepared)
    }

    pub(crate) fn prepare(
        &self,
        ir: &MockIr,
        field: FieldId,
        topic: &str,
    ) -> Result<PreparedBinding, PublicationError> {
        validate_topic(topic)?;
        self.prepare_topic(ir, field, topic)
    }

    /// Empty internal selector is sealed host authority, never an exact topic.
    pub(crate) fn prepare_all_topics(
        &self,
        ir: &MockIr,
        field: FieldId,
    ) -> Result<PreparedBinding, PublicationError> {
        self.prepare_topic(ir, field, "")
    }

    fn prepare_topic(
        &self,
        ir: &MockIr,
        field: FieldId,
        topic: &str,
    ) -> Result<PreparedBinding, PublicationError> {
        let (parent, name) = super::batch::coordinate_parts(ir, field)?;
        let bytes = (parent.len() + 1 + name.len() + topic.len())
            .checked_mul(2)
            .and_then(|bytes| {
                bytes.checked_add(size_of::<BindingEntry>() + size_of::<TopicEntry>())
            })
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let retention = self
            .retention
            .reserve(bytes, usize::MAX)
            .map_err(|_| PublicationError::CaptureLimit)?;
        let coordinate = owned_coordinate(ir, field)?;
        Ok(PreparedBinding {
            owner: Arc::clone(&self.owner),
            topic: owned(topic)?,
            topic_entry: owned(topic)?,
            coordinate,
            coordinate_entry: owned_coordinate(ir, field)?,
            port: SourcePort::with_wake(
                self.limits,
                self.retention.clone(),
                Some(self.wake.clone()),
            )?,
            retention,
            sequence: None,
        })
    }
}

fn metadata_charge(topic: usize, coordinate: usize) -> Result<usize, PublicationError> {
    topic
        .checked_add(coordinate)
        .and_then(|bytes| bytes.checked_mul(2))
        .and_then(|bytes| bytes.checked_add(BINDING_BOOKKEEPING_BYTES))
        .ok_or(PublicationError::ArithmeticOverflow)
}

impl TopicIndex {
    #[cfg(test)]
    pub(super) fn registration_owner(&self) -> RegistrationOwner {
        let limits = crate::events::SourceLimits::default();
        RegistrationOwner {
            owner: Arc::clone(&self.owner),
            limits,
            retention: crate::events::retention::Retention::new(limits.max_retained_bytes()),
            wake: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn prepare_binding(
        &self,
        ir: &MockIr,
        field: FieldId,
        topic: &str,
    ) -> Result<PreparedBinding, PublicationError> {
        if self.closed {
            return Err(PublicationError::StagingClosed);
        }
        self.registration_owner().prepare(ir, field, topic)
    }
}

impl super::PublicationStore {
    pub(crate) fn registration_owner(&self) -> RegistrationOwner {
        let (limits, retention) = self.source_policy();
        RegistrationOwner {
            owner: self.topics.owner(),
            limits,
            retention,
            wake: self.delivery_wake(),
        }
    }

    pub(crate) fn register_source(
        &mut self,
        prepared: PreparedBinding,
        overflow: chigiri::SubscriptionOverflow,
    ) -> Result<RegistrationCommit, RegistrationFailure> {
        if let Err(error) = prepared.port.configure(overflow) {
            return Err(RegistrationFailure { error, prepared });
        }
        self.topics.register(prepared).map(|outcome| {
            let generation = outcome.binding.generation;
            RegistrationCommit {
                subscription: super::source::RegisteredSubscription::new(outcome.binding, overflow),
                generation,
                retired_topic_candidate: outcome.retired_topic_candidate,
            }
        })
    }
}

impl RegistrationCommit {
    #[cfg(test)]
    pub(in crate::publication) fn finish_at(
        self,
        now: std::time::Instant,
    ) -> super::source::RegisteredSubscription {
        self.subscription.activate_at(now);
        self.finish()
    }

    pub(crate) fn source_delivery(&self) -> Arc<chigiri::__private::HostSourceDelivery> {
        self.subscription.source_delivery()
    }
    pub(crate) const fn generation(&self) -> u64 {
        self.generation.0
    }

    /// Dispose duplicate topic metadata after the store fence is released.
    pub(crate) fn finish(self) -> super::source::RegisteredSubscription {
        let subscription = self.finish_deferred();
        subscription.activate();
        subscription
    }

    /// Named startup finalizes the whole set before activating any clock.
    pub(crate) fn finish_deferred(self) -> super::source::RegisteredSubscription {
        drop(self.retired_topic_candidate);
        self.subscription
    }
}

impl RegistrationFailure {
    /// Dispose prepared metadata after the store fence is released.
    pub(crate) fn finish(self) -> PublicationError {
        drop(self.prepared);
        self.error
    }
}

impl RegisteredBinding {
    #[cfg(test)]
    pub(super) const fn generation(&self) -> u64 {
        self.generation.0
    }

    #[cfg(test)]
    pub(super) const fn registration_sequence(&self) -> u64 {
        self.registration.0
    }

    pub(super) const fn port(&self) -> &Arc<SourcePort> {
        &self.port
    }

    #[cfg(test)]
    pub(super) fn same_owner(&self, owner: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.owner, owner)
    }
}
