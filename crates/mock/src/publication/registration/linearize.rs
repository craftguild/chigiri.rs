//! Linearize one prepared source without allocation, wakeup or capture disposal.

use super::{PreparedBinding, RegisteredBinding, RegistrationFailure, RegistrationOutcome};
use crate::publication::{
    PublicationError,
    topics::{BindingEntry, TopicEntry, TopicIndex},
};
use std::sync::Arc;

impl TopicIndex {
    pub(in crate::publication) fn register(
        &mut self,
        prepared: PreparedBinding,
    ) -> Result<RegistrationOutcome, RegistrationFailure> {
        let charge = match super::metadata_charge(prepared.topic.len(), prepared.coordinate.len()) {
            Ok(charge) => charge,
            Err(error) => return Err(RegistrationFailure { error, prepared }),
        };
        let valid_owner = Arc::ptr_eq(&prepared.owner, &self.owner);
        let topic_exists = prepared.sequence.is_some()
            || self.topics.iter().any(|entry| {
                entry.topic.as_ref() == prepared.topic.as_ref()
                    && entry.coordinate == prepared.coordinate
            });
        let error = if !valid_owner {
            Some(PublicationError::ForeignOwner)
        } else if self.closed {
            Some(PublicationError::StagingClosed)
        } else if self.bindings.len() >= self.max_bindings
            || (!topic_exists && self.topics.len() >= self.max_bindings)
        {
            Some(PublicationError::BindingCapacity)
        } else {
            None
        };
        if let Some(error) = error {
            return Err(RegistrationFailure { error, prepared });
        }
        if let Err(error) = prepared.port.admit(&self.admission) {
            return Err(RegistrationFailure { error, prepared });
        }
        let (registration, generation) = match self.sequences.registration() {
            Ok(values) => values,
            Err(error) => return Err(RegistrationFailure { error, prepared }),
        };
        let PreparedBinding {
            topic,
            topic_entry,
            coordinate,
            coordinate_entry,
            port,
            retention,
            sequence,
            ..
        } = prepared;
        let retired_topic_candidate = if topic_exists {
            Some((topic_entry, coordinate_entry))
        } else {
            self.topics.push(TopicEntry {
                topic: topic_entry,
                coordinate: coordinate_entry,
                highest_lost: 0,
            });
            None
        };
        self.bindings.push(BindingEntry {
            topic,
            coordinate,
            generation,
            registration,
            _charge: charge,
            port: Arc::downgrade(&port),
            input: port.input_usage(),
            sequence,
            cursor_sequence: registration.0,
            cursor_publication: 0,
            _retention: retention,
        });
        Ok(RegistrationOutcome {
            binding: RegisteredBinding {
                #[cfg(test)]
                owner: Arc::clone(&self.owner),
                generation,
                #[cfg(test)]
                registration,
                port,
            },
            retired_topic_candidate,
        })
    }
}
