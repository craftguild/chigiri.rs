//! Select one eligible binding at a time without fan-out allocation.

use super::TopicIndex;
#[cfg(test)]
use super::topics::BindingTarget;
#[cfg(test)]
use super::{CapturedPublication, PublicationError, StagedBatch};

pub(super) struct IncrementalDispatcher {
    #[cfg(test)]
    current: Option<StagedBatch>,
    #[cfg(test)]
    publication: usize,
    after_registration: u64,
    remaining: usize,
}

#[cfg(test)]
pub(super) enum DispatchAction<'a> {
    Deliver {
        publication: &'a CapturedPublication,
        target: BindingTarget,
    },
    Lost {
        publication: &'a CapturedPublication,
        target: BindingTarget,
    },
    PublicationComplete,
    BatchComplete(StagedBatch),
    Idle,
}

impl IncrementalDispatcher {
    pub(super) const fn new() -> Self {
        Self {
            #[cfg(test)]
            current: None,
            #[cfg(test)]
            publication: 0,
            after_registration: 0,
            remaining: 0,
        }
    }

    pub(super) fn select_binding(&mut self, topics: &TopicIndex) -> Option<usize> {
        if self.remaining == 0 {
            self.after_registration = 0;
            self.remaining = topics.bindings.len();
            if self.remaining == 0 {
                return None;
            }
        }
        let selected = topics
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| binding.registration.0 > self.after_registration)
            .min_by_key(|(_, binding)| binding.registration.0)
            .map(|(index, _)| index);
        let Some(index) = selected else {
            self.remaining = 0;
            return None;
        };
        self.remaining -= 1;
        self.after_registration = topics.bindings[index].registration.0;
        Some(index)
    }

    pub(super) const fn runnable(&self) -> bool {
        self.remaining > 0
    }

    #[cfg(test)]
    pub(super) fn accept(&mut self, batch: StagedBatch) -> Result<(), StagedBatch> {
        if self.current.is_some() {
            return Err(batch);
        }
        self.current = Some(batch);
        self.publication = 0;
        self.after_registration = 0;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn next<'a>(
        &'a mut self,
        topics: &TopicIndex,
    ) -> Result<DispatchAction<'a>, PublicationError> {
        let Some(staged) = self.current.as_ref() else {
            return Ok(DispatchAction::Idle);
        };
        if !topics.owns_sequence(staged.sequence()) {
            return Err(PublicationError::ForeignOwner);
        }
        if self.publication >= staged.batch().publications().len() {
            return Ok(match self.current.take() {
                Some(batch) => DispatchAction::BatchComplete(batch),
                None => DispatchAction::Idle,
            });
        }
        let Some(staged) = self.current.as_ref() else {
            return Ok(DispatchAction::Idle);
        };
        let Some(publication) = staged.batch().publications().get(self.publication) else {
            return Ok(DispatchAction::Idle);
        };
        let Some(target) = topics.next_publication_binding(
            publication,
            self.after_registration,
            staged.sequence().value(),
        ) else {
            self.publication += 1;
            self.after_registration = 0;
            return Ok(DispatchAction::PublicationComplete);
        };
        self.after_registration = target.registration;
        if target.lost {
            Ok(DispatchAction::Lost {
                publication,
                target,
            })
        } else {
            Ok(DispatchAction::Deliver {
                publication,
                target,
            })
        }
    }

    #[cfg(test)]
    pub(super) fn shutdown(&mut self) -> Option<StagedBatch> {
        self.publication = 0;
        self.after_registration = 0;
        self.current.take()
    }

    #[cfg(test)]
    pub(super) fn ensure_idle(&self) -> Result<(), PublicationError> {
        if self.current.is_some() {
            Err(PublicationError::DispatcherBusy)
        } else {
            Ok(())
        }
    }
}
