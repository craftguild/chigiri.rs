//! Short policy transitions serialized with exact body-acceptance timestamps.

use std::task::Context;

use super::payload::ResponsePayload;
use super::{CurrentFrame, Entry, HttpStreamingError, WireBytes, interval_ns};
use crate::subscription::outbox::{SubscriptionEventReservation, SubscriptionOutboxSignal};

impl Entry {
    fn notifications(&mut self, context: &mut Context<'_>) -> Result<bool, HttpStreamingError> {
        let notifications = self.port.notifications(context)?;
        if notifications.closed {
            return Ok(true);
        }
        if !self.wire_closed {
            if let Some((length, now)) = notifications.accepted {
                match &mut self.current {
                    Some(CurrentFrame::Event(progress)) => {
                        progress
                            .accept(length, now)
                            .map_err(HttpStreamingError::Delivery)?;
                    }
                    Some(CurrentFrame::RetiredEvent(progress)) => {
                        progress
                            .accept(length, now)
                            .map_err(HttpStreamingError::Delivery)?;
                    }
                    _ => {}
                }
            }
            if notifications.released
                && let Some(CurrentFrame::Event(progress)) = self.current.take()
            {
                self.outbox
                    .finish_send(progress, self.clock.now()?)
                    .map_err(HttpStreamingError::Delivery)?;
            }
            if self.current.is_some() != notifications.live {
                return Err(HttpStreamingError::Invariant);
            }
        }
        Ok(false)
    }

    pub(super) fn update_policy(
        &mut self,
        context: &mut Context<'_>,
    ) -> Result<(), HttpStreamingError> {
        let port = std::sync::Arc::clone(&self.port);
        let timeline = port
            .timeline
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        let closed = self.notifications(context)?;
        if !closed && let Some(CurrentFrame::RetiredEvent(progress)) = &self.current {
            progress
                .on_timer(self.clock.now()?)
                .map_err(HttpStreamingError::Delivery)?;
        }
        if !closed && !self.stopping && self.terminal.is_none() {
            let now = self.clock.now()?;
            if !self.draining && now >= self.heartbeat_at {
                self.heartbeat
                    .mark_due()
                    .map_err(HttpStreamingError::Control)?;
                self.heartbeat_at = now
                    .checked_add(interval_ns(&self.configuration)?)
                    .ok_or(HttpStreamingError::Clock)?;
            }
            match self.outbox.on_timer(now) {
                Ok(()) | Err(SubscriptionOutboxSignal::Wait) => {}
                Err(signal) => return Err(HttpStreamingError::Delivery(signal)),
            }
        }
        drop(timeline);
        if closed {
            self.cancel(None);
        }
        Ok(())
    }

    pub(super) fn produce(
        &mut self,
        context: &mut Context<'_>,
        candidate: Option<(
            SubscriptionEventReservation,
            chigiri_runtime::GraphQlResponse,
        )>,
    ) -> Result<(), HttpStreamingError> {
        // Both payload owners precede the guard, including on unwind. Do not
        // rebind either owned response inside the locked encoding branches.
        let (reservation, response) = match candidate {
            Some((reservation, response)) => (Some(reservation), Some(response)),
            None => (None, None),
        };
        let port = std::sync::Arc::clone(&self.port);
        let response = ResponsePayload::new(response, &port, false);
        let mut discarded = ResponsePayload::new(None, &port, true);
        let control = port.control();
        let (timeline, gate, closed) = loop {
            let timeline = port
                .timeline
                .lock()
                .map_err(|_| HttpStreamingError::Invariant)?;
            // Waker replacement/destruction precedes the endpoint state lock.
            let closed = self.notifications(context)?;
            let gate = match control
                .as_ref()
                .map(|control| control.emission_gate())
                .transpose()
            {
                Ok(gate) => gate,
                Err(_) => {
                    drop(timeline);
                    self.cancel_managed();
                    return Ok(());
                }
            };
            // Shutdown can race a source-cancelled result after Running was
            // sampled. A stopped owner cannot reinitialize graceful policy.
            if !self.draining
                && !self.stopping
                && gate.as_ref().is_some_and(|gate| {
                    matches!(gate.phase(), crate::managed::StopPhase::Graceful { .. })
                })
            {
                drop(gate);
                drop(timeline);
                self.begin_graceful();
                continue;
            }
            break (timeline, gate, closed);
        };
        if reservation.is_some() && self.draining && !self.graceful_event {
            drop(gate);
            drop(timeline);
            return Ok(());
        }
        if closed || self.wire_closed {
            drop(timeline);
            drop(gate);
            if closed {
                self.cancel(None);
            }
            return Ok(());
        }
        let now = self.clock.now()?;
        if let Some(reservation) = reservation {
            #[cfg(test)]
            self.port.run_encode_fault_for_test(false);
            let result = self
                .outbox
                .encode_and_enqueue(
                    reservation,
                    response.value.as_ref().expect("one event response"),
                    now,
                )
                .map_err(HttpStreamingError::Delivery);
            // The candidate owns only framework values, but its potentially
            // large allocation is still destroyed outside the timeline lock.
            if result.is_ok()
                && let Some(events) = &self.events
            {
                events.activity().finish();
            }
            drop(timeline);
            drop(gate);
            drop(response);
            result?;
            if self.draining {
                self.graceful_event = false;
            }
            return self.produce(context, None);
        }
        if self.current.is_none()
            && self.outbox.is_empty()
            && let Some((reservation, response)) = self.pending_terminal.take()
        {
            discarded.value = response.map(|response| response.into_response());
            #[cfg(test)]
            self.port.run_encode_fault_for_test(true);
            let staged = self
                .outbox
                .finish_http_source(reservation)
                .map_err(HttpStreamingError::Delivery)
                .and_then(|terminal| {
                    match &discarded.value {
                        Some(response) => terminal.error(response),
                        None => terminal.complete(),
                    }
                    .map_err(HttpStreamingError::Control)
                });
            match staged {
                Ok(staged) => self.terminal = Some(staged),
                Err(error) => {
                    drop(timeline);
                    drop(gate);
                    drop(discarded);
                    return Err(error);
                }
            }
        }
        let frame = match self.next_frame(now) {
            Ok(frame) => frame,
            Err(error) => {
                drop(timeline);
                drop(gate);
                drop(discarded);
                return Err(error);
            }
        };
        let mut demand = None;
        if !self.stopping
            && !self.draining
            && !self.source_done
            && !self.demanded
            && self.pending_terminal.is_none()
            && self.terminal.is_none()
        {
            match self.outbox.reserve_before_poll(now) {
                Ok(reservation) => demand = Some(reservation),
                Err(SubscriptionOutboxSignal::Wait) => {}
                Err(signal) => {
                    drop(timeline);
                    drop(gate);
                    drop(discarded);
                    return Err(HttpStreamingError::Delivery(signal));
                }
            }
        }
        let finished = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.is_finished());
        drop(timeline);
        drop(gate);
        drop(discarded);
        if let Some(frame) = frame {
            match self.port.offer(frame, self.physical.clone()) {
                Ok(()) => {}
                Err(HttpStreamingError::Closed) => {
                    self.cancel(None);
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        if let Some(reservation) = demand {
            #[cfg(test)]
            self.port.run_before_demand_for_test();
            // Close and the actual non-polling demand-port commit share one
            // authority. Defer the worker wake until this timeline unlocks.
            let timeline = port
                .timeline
                .lock()
                .map_err(|_| HttpStreamingError::Invariant)?;
            let gate = match &control {
                Some(control) => match control.registration_gate() {
                    Ok(gate) => Some(gate),
                    Err(_) => return Ok(()),
                },
                None => None,
            };
            if self.port.is_closed()? {
                drop(timeline);
                drop(gate);
                self.cancel(None);
                return Ok(());
            }
            #[cfg(test)]
            self.port.run_after_demand_check_for_test();
            let Some(events) = &mut self.events else {
                return Err(HttpStreamingError::Worker);
            };
            let result = events.request_next_deferred(reservation);
            drop(timeline);
            drop(gate);
            let wake = result.map_err(|_| HttpStreamingError::Worker)?;
            self.demanded = true;
            if let Some(wake) = wake {
                wake.wake();
            }
        }
        if finished {
            self.wire_closed = true;
            self.port.finish(None);
        }
        Ok(())
    }

    fn next_frame(&mut self, now: u64) -> Result<Option<WireBytes>, HttpStreamingError> {
        if self.current.is_some() {
            return Ok(None);
        }
        if let Some(terminal) = &mut self.terminal {
            return terminal
                .begin_send()
                .map(|frame| {
                    frame.map(|frame| {
                        self.current = Some(CurrentFrame::Control);
                        WireBytes::Control(frame)
                    })
                })
                .map_err(HttpStreamingError::Control);
        }
        if self.stopping {
            return Ok(None);
        }
        match self.outbox.begin_send(now) {
            Ok(Some((frame, progress))) => {
                self.current = Some(CurrentFrame::Event(progress));
                return Ok(Some(WireBytes::Event(frame)));
            }
            Ok(None) | Err(SubscriptionOutboxSignal::Wait) => {}
            Err(signal) => return Err(HttpStreamingError::Delivery(signal)),
        }
        if self.pending_terminal.is_some() || self.draining {
            return Ok(None);
        }
        self.heartbeat
            .try_begin(self.outbox.has_queued_events(), false)
            .map(|frame| {
                frame.map(|frame| {
                    self.current = Some(CurrentFrame::Control);
                    WireBytes::Control(frame)
                })
            })
            .map_err(HttpStreamingError::Control)
    }
}
