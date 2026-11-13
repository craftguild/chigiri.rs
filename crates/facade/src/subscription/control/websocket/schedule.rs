//! Allocation-free connection-writer arbitration at complete frame boundaries.

use super::SubscriptionControlSignal;

const MAX_ORDINARY_RUN: u8 = 8;

/// The closed priority class fixed by a control's protocol role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketControlClass {
    /// Connection acknowledgment or operation error/completion.
    Priority,
    /// Ordinary JSON ping or pong.
    Ordinary,
}

/// One candidate turn, which does not by itself mutate writer fairness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketWriterTurn {
    /// The oldest queued control in the chosen class.
    Control(WebSocketControlClass),
    /// One event from the selected slot in the connection's bounded slot table.
    Event(usize),
}

/// One connection's cursor and ordinary-control run, with no queues or tasks.
///
/// Call only between complete socket frames. The connection owns the bounded
/// readiness table and the sole physical writer; this value owns neither.
/// Selection is read-only, so a failed frame acquisition or a revoked event
/// cannot grant another control run. Record a turn only after the matching
/// immutable frame passes its emission fence and is acquired by that writer.
#[derive(Debug, Default)]
pub struct WebSocketWriterSchedule {
    next_event: usize,
    ordinary_run: u8,
}

impl WebSocketWriterSchedule {
    /// Chooses priority first, then at most eight ordinary controls per event.
    pub fn select(
        &self,
        event_ready: &[bool],
        priority_ready: bool,
        ordinary_ready: bool,
    ) -> Option<WebSocketWriterTurn> {
        if priority_ready {
            return Some(WebSocketWriterTurn::Control(
                WebSocketControlClass::Priority,
            ));
        }
        let event = self.next_ready(event_ready);
        if ordinary_ready && (self.ordinary_run < MAX_ORDINARY_RUN || event.is_none()) {
            return Some(WebSocketWriterTurn::Control(
                WebSocketControlClass::Ordinary,
            ));
        }
        event.map(WebSocketWriterTurn::Event)
    }

    /// Commits an acquired control without letting priority or idle reset a run.
    pub fn control_acquired(&mut self, class: WebSocketControlClass) {
        if class == WebSocketControlClass::Ordinary {
            self.ordinary_run = self.ordinary_run.saturating_add(1).min(MAX_ORDINARY_RUN);
        }
    }

    /// Commits one acquired event and advances to the following bounded slot.
    pub fn event_acquired(
        &mut self,
        slot: usize,
        slot_count: usize,
    ) -> Result<(), SubscriptionControlSignal> {
        if slot >= slot_count {
            return Err(SubscriptionControlSignal::Accounting);
        }
        self.next_event = (slot + 1) % slot_count;
        self.ordinary_run = 0;
        Ok(())
    }

    fn next_ready(&self, event_ready: &[bool]) -> Option<usize> {
        if event_ready.is_empty() {
            return None;
        }
        let start = self.next_event % event_ready.len();
        (start..event_ready.len())
            .chain(0..start)
            .find(|index| event_ready[*index])
    }
}

#[cfg(test)]
mod tests;
