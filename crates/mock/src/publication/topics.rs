//! Server-local registration, loss markers, and shared commit ordering.

use std::sync::Arc;

use super::limits::{MAX_ACTIVE_SUBSCRIPTIONS, MAX_SAFE_SEQUENCE};
use super::sequence::{Generation, RegistrationSequence, SequenceOwner};
use super::source::SourcePort;
use super::{CapturedBatch, CommitSequence, PublicationError};

pub(super) use super::registration::RegisteredBinding;

pub(super) struct TopicEntry {
    pub(super) topic: Box<str>,
    pub(super) coordinate: Box<str>,
    pub(super) highest_lost: u64,
}

pub(super) struct BindingEntry {
    pub(super) topic: Box<str>,
    pub(super) coordinate: Box<str>,
    pub(super) generation: Generation,
    pub(super) registration: RegistrationSequence,
    pub(super) _charge: usize,
    pub(super) port: std::sync::Weak<SourcePort>,
    pub(super) input: Arc<super::source::accounting::Usage>,
    pub(super) sequence: Option<Box<super::timed::TimedSequence>>,
    pub(super) cursor_sequence: u64,
    pub(super) cursor_publication: usize,
    pub(super) _retention: crate::events::retention::Reservation,
}

pub(super) struct TopicIndex {
    pub(super) owner: Arc<()>,
    pub(super) sequences: SequenceOwner,
    pub(super) max_bindings: usize,
    pub(super) topics: Vec<TopicEntry>,
    pub(super) bindings: Vec<BindingEntry>,
    pub(super) closed: bool,
    pub(super) admission: super::source::admission::Admission,
}

pub(super) struct RetiredBinding {
    _topic: Box<str>,
    _coordinate: Box<str>,
    _retired_topic: Option<(Box<str>, Box<str>)>,
    _input: Arc<super::source::accounting::Usage>,
    _sequence: Option<Box<super::timed::TimedSequence>>,
    _retention: crate::events::retention::Reservation,
}

pub(crate) struct RetiredIndex {
    _topics: Vec<TopicEntry>,
    _bindings: Vec<BindingEntry>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BindingTarget {
    pub(super) generation: u64,
    pub(super) registration: u64,
    pub(super) charge: usize,
    pub(super) lost: bool,
}

impl TopicIndex {
    pub(super) fn new(max_bindings: usize) -> Result<Self, PublicationError> {
        Self::with_sequence_limit(max_bindings, MAX_SAFE_SEQUENCE)
    }

    pub(super) fn owner(&self) -> Arc<()> {
        Arc::clone(&self.owner)
    }

    #[cfg(test)]
    pub(super) fn owns_sequence(&self, sequence: &CommitSequence) -> bool {
        sequence.same_owner(&self.owner)
    }

    fn with_sequence_limit(
        max_bindings: usize,
        sequence_limit: u64,
    ) -> Result<Self, PublicationError> {
        if max_bindings == 0 || max_bindings > MAX_ACTIVE_SUBSCRIPTIONS {
            return Err(PublicationError::InvalidConfiguration);
        }
        let sequences = SequenceOwner::with_limit(sequence_limit);
        let owner = Arc::clone(sequences.identity());
        let mut topics = Vec::new();
        topics
            .try_reserve_exact(max_bindings)
            .map_err(|_| PublicationError::Allocation)?;
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(max_bindings)
            .map_err(|_| PublicationError::Allocation)?;
        Ok(Self {
            owner,
            sequences,
            max_bindings,
            topics,
            bindings,
            closed: false,
            admission: super::source::admission::Admission::new(max_bindings),
        })
    }

    pub(super) fn commit_sequence(&mut self) -> Result<CommitSequence, PublicationError> {
        if self.closed {
            return Err(PublicationError::StagingClosed);
        }
        self.sequences.commit()
    }

    pub(super) fn check_admission(&self) -> Result<(), PublicationError> {
        if self.closed {
            return Err(PublicationError::StagingClosed);
        }
        self.sequences.check_next()
    }

    pub(super) fn close_admission(&mut self) {
        self.closed = true;
    }

    pub(super) fn raise_loss(
        &mut self,
        sequence: &CommitSequence,
        batch: &CapturedBatch,
    ) -> Result<(), PublicationError> {
        if !sequence.same_owner(&self.owner) {
            return Err(PublicationError::ForeignOwner);
        }
        for publication in batch.publications() {
            for coordinate in publication.coordinates() {
                for topic in self.topics.iter_mut().filter(|entry| {
                    (entry.topic.is_empty() || entry.topic.as_ref() == publication.topic())
                        && entry.coordinate.as_ref() == coordinate
                }) {
                    topic.highest_lost = topic.highest_lost.max(sequence.value());
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn unregister(
        &mut self,
        binding: RegisteredBinding,
    ) -> Result<RetiredBinding, (PublicationError, RegisteredBinding)> {
        if !binding.same_owner(&self.owner) {
            return Err((PublicationError::ForeignOwner, binding));
        }
        let Some(index) = self
            .bindings
            .iter()
            .position(|entry| entry.generation.0 == binding.generation())
        else {
            return Err((PublicationError::UnknownBinding, binding));
        };
        Ok(self.retire_index(index))
    }

    pub(super) fn retire_index(&mut self, index: usize) -> RetiredBinding {
        let topic = self.bindings[index].topic.as_ref();
        let last_binding = self.bindings[index].sequence.is_none()
            && !self.bindings.iter().enumerate().any(|(other, entry)| {
                other != index
                    && entry.sequence.is_none()
                    && entry.topic.as_ref() == topic
                    && entry.coordinate == self.bindings[index].coordinate
            });
        let topic_index = if last_binding {
            self.topics.iter().position(|entry| {
                entry.topic.as_ref() == topic && entry.coordinate == self.bindings[index].coordinate
            })
        } else {
            None
        };
        let retired = self.bindings.swap_remove(index);
        let retired_topic = topic_index.map(|index| {
            let entry = self.topics.swap_remove(index);
            (entry.topic, entry.coordinate)
        });
        RetiredBinding {
            _topic: retired.topic,
            _coordinate: retired.coordinate,
            _retired_topic: retired_topic,
            _retention: retired._retention,
            _input: retired.input,
            _sequence: retired.sequence,
        }
    }

    pub(super) fn shutdown(&mut self) -> RetiredIndex {
        self.closed = true;
        RetiredIndex {
            _topics: std::mem::take(&mut self.topics),
            _bindings: std::mem::take(&mut self.bindings),
        }
    }

    #[cfg(test)]
    pub(super) fn next_binding(
        &self,
        topic: &str,
        coordinate: &str,
        after: u64,
        before: u64,
    ) -> Option<BindingTarget> {
        self.matching_binding(topic, |actual| actual == coordinate, after, before)
    }

    #[cfg(test)]
    pub(super) fn next_publication_binding(
        &self,
        publication: &super::CapturedPublication,
        after: u64,
        before: u64,
    ) -> Option<BindingTarget> {
        self.matching_binding(
            publication.topic(),
            |actual| publication.matches_coordinate(actual),
            after,
            before,
        )
    }

    #[cfg(test)]
    fn matching_binding(
        &self,
        topic: &str,
        matches: impl Fn(&str) -> bool,
        after: u64,
        before: u64,
    ) -> Option<BindingTarget> {
        let binding = self
            .bindings
            .iter()
            .filter(|binding| {
                binding.sequence.is_none()
                    && (binding.topic.is_empty() || binding.topic.as_ref() == topic)
                    && matches(&binding.coordinate)
                    && binding.registration.0 > after
                    && binding.registration.0 < before
            })
            .min_by_key(|binding| binding.registration)?;
        let highest_lost = self
            .topics
            .iter()
            .find(|entry| entry.topic == binding.topic && entry.coordinate == binding.coordinate)
            .map_or(0, |entry| entry.highest_lost);
        Some(BindingTarget {
            generation: binding.generation.0,
            registration: binding.registration.0,
            charge: binding._charge,
            lost: highest_lost > binding.registration.0,
        })
    }

    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize) {
        (self.topics.len(), self.bindings.len())
    }

    #[cfg(test)]
    pub(super) fn binding_charge(&self, generation: u64) -> Option<usize> {
        self.bindings
            .iter()
            .find(|binding| binding.generation.0 == generation)
            .map(|binding| binding._charge)
    }
}

#[cfg(test)]
impl TopicIndex {
    pub(super) fn with_test_sequence_limit(
        max_bindings: usize,
        sequence_limit: u64,
    ) -> Result<Self, PublicationError> {
        Self::with_sequence_limit(max_bindings, sequence_limit)
    }
}
