//! Select bounded publication or timer work under the store linearization lock.

use super::{Plan, PublicationError, PublicationStore, RetainedBatch, Termination};
use std::time::Instant;

pub(super) fn reclaim_or_pressure(
    store: &mut PublicationStore,
) -> Result<Option<Plan>, PublicationError> {
    let pressure = store.staging.pressured()?;
    // Cursors can finish while source inputs retain the batch reservation.
    // Apply policy to a real retainer outside the fence, then recheck the
    // exact failed reservation before selecting any further recovery.
    if pressure
        && let Some(port) = store
            .topics
            .bindings
            .iter()
            .find(|binding| binding.sequence.is_none() && binding.input.retains_input())
            .and_then(|binding| binding.port.upgrade())
    {
        return Ok(Some(Plan::Pressure(port)));
    }
    let Some(front) = store.staging.front()? else {
        return Ok(None);
    };
    let blocker = store
        .topics
        .bindings
        .iter()
        .enumerate()
        .filter(|(_, binding)| {
            binding.sequence.is_none()
                && binding.registration.0 < front.sequence
                && (binding.cursor_sequence < front.sequence
                    || (binding.cursor_sequence == front.sequence
                        && binding.cursor_publication < front.publications))
        })
        .min_by_key(|(_, binding)| {
            (
                binding.cursor_sequence,
                binding.cursor_publication,
                binding.registration.0,
            )
        })
        .map(|(index, _)| index);
    match (pressure, blocker) {
        (true, Some(index)) => {
            let port = store.topics.bindings[index].port.upgrade();
            let retired = store.topics.retire_index(index);
            Ok(Some(Plan::Retire {
                port,
                reason: Termination::Pressure,
                retired,
            }))
        }
        (_, None) => Ok(store.staging.pop_front(front.sequence)?.map(Plan::Reclaim)),
        _ => Ok(None),
    }
}

pub(super) fn plan_binding(
    store: &mut PublicationStore,
    index: usize,
    now: Instant,
) -> Result<Plan, PublicationError> {
    let Some(port) = store.topics.bindings[index].port.upgrade() else {
        return Ok(Plan::Retire {
            port: None,
            reason: Termination::Shutdown,
            retired: store.topics.retire_index(index),
        });
    };
    if port.is_closed() {
        return Ok(Plan::Retire {
            port: Some(port),
            reason: Termination::Shutdown,
            retired: store.topics.retire_index(index),
        });
    }
    if let Some(sequence) = &mut store.topics.bindings[index].sequence {
        return match sequence.tick(port.activated_at(), now) {
            Ok(Some(tick)) => Ok(Plan::Tick {
                port,
                event: tick.event,
                complete: tick.complete,
            }),
            Ok(None) => Ok(Plan::Scanned(port)),
            Err(PublicationError::SequenceExhausted | PublicationError::ArithmeticOverflow) => {
                Ok(Plan::Retire {
                    port: Some(port),
                    reason: Termination::Sequence,
                    retired: store.topics.retire_index(index),
                })
            }
            Err(error) => Err(error),
        };
    }
    let binding = &store.topics.bindings[index];
    let lost = store
        .topics
        .topics
        .iter()
        .find(|topic| {
            topic.topic.as_ref() == binding.topic.as_ref() && topic.coordinate == binding.coordinate
        })
        .is_some_and(|topic| topic.highest_lost > binding.registration.0);
    if lost {
        return Ok(Plan::Retire {
            port: Some(port),
            reason: Termination::Lost,
            retired: store.topics.retire_index(index),
        });
    }
    let Some(candidate) = store
        .staging
        .candidate_after(binding.cursor_sequence, binding.cursor_publication)?
    else {
        return Ok(Plan::Scanned(port));
    };
    let publication = candidate
        .retained
        .publication(candidate.publication)
        .ok_or(PublicationError::Invariant)?;
    let next_publication = candidate
        .publication
        .checked_add(1)
        .ok_or(PublicationError::ArithmeticOverflow)?;
    if (!binding.topic.is_empty() && publication.topic() != binding.topic.as_ref())
        || !publication.matches_coordinate(&binding.coordinate)
    {
        let binding = &mut store.topics.bindings[index];
        binding.cursor_sequence = candidate.sequence;
        binding.cursor_publication = next_publication;
        return Ok(Plan::Advanced);
    }
    let event = RetainedBatch::event(&candidate.retained, candidate.publication)?;
    Ok(Plan::Offer {
        port,
        generation: binding.generation.0,
        expected_sequence: binding.cursor_sequence,
        expected_publication: binding.cursor_publication,
        next_sequence: candidate.sequence,
        next_publication,
        event,
    })
}

pub(super) fn commit_cursor(
    store: &mut PublicationStore,
    generation: u64,
    expected_sequence: u64,
    expected_publication: usize,
    next_sequence: u64,
    next_publication: usize,
) -> bool {
    let Some(binding) = store
        .topics
        .bindings
        .iter_mut()
        .find(|binding| binding.generation.0 == generation)
    else {
        return false;
    };
    if binding.cursor_sequence != expected_sequence
        || binding.cursor_publication != expected_publication
    {
        return false;
    }
    binding.cursor_sequence = next_sequence;
    binding.cursor_publication = next_publication;
    true
}
