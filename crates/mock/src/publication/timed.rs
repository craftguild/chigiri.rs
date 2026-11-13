//! Select one newest due position using a fixed monotonic origin and constant work.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{CapturedEvent, MAX_SAFE_SEQUENCE, PublicationError, SequenceDefinition};

#[cfg(test)]
mod tests;

pub(super) struct TimedSequence {
    definition: Arc<SequenceDefinition>,
    origin: Option<Instant>,
    last_tick: u64,
}

pub(super) struct Tick {
    pub(super) event: CapturedEvent,
    pub(super) complete: bool,
}

impl TimedSequence {
    pub(super) fn new(definition: Arc<SequenceDefinition>) -> Self {
        Self {
            definition,
            origin: None,
            last_tick: 0,
        }
    }

    pub(super) fn tick(
        &mut self,
        activation: Option<Instant>,
        now: Instant,
    ) -> Result<Option<Tick>, PublicationError> {
        let Some(activation) = activation else {
            return Ok(None);
        };
        let origin = *self.origin.get_or_insert(activation);
        if activation != origin {
            return Err(PublicationError::Invariant);
        }
        let due = now.saturating_duration_since(origin).as_millis()
            / u128::from(self.definition.interval_ms());
        self.select(due)
    }

    fn select(&mut self, due: u128) -> Result<Option<Tick>, PublicationError> {
        let due = if self.definition.looping() {
            due
        } else {
            due.min(self.definition.len() as u128)
        };
        if due <= u128::from(self.last_tick) {
            return Ok(None);
        }
        if due > u128::from(MAX_SAFE_SEQUENCE) {
            return Err(PublicationError::SequenceExhausted);
        }
        let tick = u64::try_from(due).map_err(|_| PublicationError::ArithmeticOverflow)?;
        let index = usize::try_from((tick - 1) % self.definition.len() as u64)
            .map_err(|_| PublicationError::ArithmeticOverflow)?;
        let event = CapturedEvent::sequence(&self.definition, index)?;
        self.last_tick = tick;
        Ok(Some(Tick {
            event,
            complete: !self.definition.looping() && tick == self.definition.len() as u64,
        }))
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        if !self.definition.looping() && self.last_tick >= self.definition.len() as u64 {
            return None;
        }
        let milliseconds = self
            .last_tick
            .checked_add(1)?
            .checked_mul(self.definition.interval_ms())?;
        self.origin?
            .checked_add(Duration::from_millis(milliseconds))
    }
}
