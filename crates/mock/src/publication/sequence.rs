//! Opaque server-local checked ordering and generation values.

use std::sync::Arc;

use super::PublicationError;
#[derive(Debug)]
pub(super) struct CommitSequence {
    owner: Arc<()>,
    value: u64,
}

impl CommitSequence {
    pub(super) const fn value(&self) -> u64 {
        self.value
    }

    pub(super) fn same_owner(&self, owner: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.owner, owner)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RegistrationSequence(pub(super) u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Generation(pub(super) u64);

pub(super) struct SequenceOwner {
    identity: Arc<()>,
    next: u64,
    next_generation: u64,
    limit: u64,
}

impl SequenceOwner {
    pub(super) fn with_limit(limit: u64) -> Self {
        Self {
            identity: Arc::new(()),
            next: 0,
            next_generation: 0,
            limit,
        }
    }

    pub(super) fn registration(
        &mut self,
    ) -> Result<(RegistrationSequence, Generation), PublicationError> {
        let sequence = checked_next(self.next, self.limit)?;
        let generation = checked_next(self.next_generation, self.limit)?;
        self.next = sequence;
        self.next_generation = generation;
        Ok((RegistrationSequence(sequence), Generation(generation)))
    }

    pub(super) fn commit(&mut self) -> Result<CommitSequence, PublicationError> {
        let value = checked_next(self.next, self.limit)?;
        self.next = value;
        Ok(CommitSequence {
            owner: Arc::clone(&self.identity),
            value,
        })
    }

    pub(super) fn check_next(&self) -> Result<(), PublicationError> {
        checked_next(self.next, self.limit).map(|_| ())
    }

    pub(super) fn identity(&self) -> &Arc<()> {
        &self.identity
    }
}

fn checked_next(current: u64, limit: u64) -> Result<u64, PublicationError> {
    let next = current
        .checked_add(1)
        .ok_or(PublicationError::SequenceExhausted)?;
    if next > limit {
        return Err(PublicationError::SequenceExhausted);
    }
    Ok(next)
}
