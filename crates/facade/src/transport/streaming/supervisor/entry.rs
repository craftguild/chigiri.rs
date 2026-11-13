//! One admitted source, policy controller, and body handoff; no extra task.

mod failure;
mod graceful;
mod payload;
mod policy;

use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_runtime::__private::{SubscriptionTerminalResponse, SubscriptionTermination};

use super::HttpStreamingError;
use super::body::{BodyPort, WireBytes};
use super::clock::StreamClock;
use crate::subscription::bridge_resources::HttpSubscriptionPhysicalLease;
use crate::subscription::control::{HttpHeartbeatStaging, HttpTerminalStaging};
use crate::subscription::driver::SubscriptionOutput;
use crate::subscription::outbox::{
    RetiredSendProgress, SubscriptionEventReservation, SubscriptionOutbox, SubscriptionSendProgress,
};
use crate::transport::streaming::worker::HttpStreamConfiguration;
use crate::transport::streaming::{HttpSubscriptionEvents, HttpSubscriptionSignal};

pub(super) struct Entry {
    port: Arc<BodyPort>,
    physical: HttpSubscriptionPhysicalLease,
    events: Option<HttpSubscriptionEvents>,
    outbox: SubscriptionOutbox,
    configuration: HttpStreamConfiguration,
    heartbeat: HttpHeartbeatStaging,
    clock: Arc<StreamClock>,
    heartbeat_at: u64,
    current: Option<CurrentFrame>,
    demanded: bool,
    draining: bool,
    graceful_event: bool,
    pending_terminal: Option<(
        SubscriptionEventReservation,
        Option<SubscriptionTerminalResponse>,
    )>,
    terminal: Option<HttpTerminalStaging>,
    stopping: bool,
    wire_closed: bool,
    source_done: bool,
    failure_selected: bool,
    failure: Option<failure::PendingFailure>,
}

enum CurrentFrame {
    Initial,
    Event(SubscriptionSendProgress),
    RetiredEvent(RetiredSendProgress),
    Control,
}

impl Entry {
    #[cfg(test)]
    pub(super) fn test_port(&self) -> &Arc<BodyPort> {
        &self.port
    }

    pub(super) fn new(
        port: Arc<BodyPort>,
        physical: HttpSubscriptionPhysicalLease,
        events: HttpSubscriptionEvents,
        outbox: SubscriptionOutbox,
        mut configuration: HttpStreamConfiguration,
        clock: Arc<StreamClock>,
    ) -> Result<Self, HttpStreamingError> {
        configuration.failure.bind_task(events.activity().clone());
        if let Some(control) = events.activity().control() {
            port.bind_control(&control);
        }
        let heartbeat_at = clock
            .now()?
            .checked_add(interval_ns(&configuration)?)
            .ok_or(HttpStreamingError::Clock)?;
        let heartbeat =
            HttpHeartbeatStaging::new(configuration.bytes.clone(), configuration.transport)
                .map_err(HttpStreamingError::Control)?;
        Ok(Self {
            port,
            physical,
            events: Some(events),
            outbox,
            configuration,
            heartbeat,
            clock,
            heartbeat_at,
            current: Some(CurrentFrame::Initial),
            demanded: false,
            draining: false,
            graceful_event: false,
            pending_terminal: None,
            terminal: None,
            stopping: false,
            wire_closed: false,
            source_done: false,
            failure_selected: false,
            failure: None,
        })
    }

    pub(super) fn poll(
        &mut self,
        context: &mut Context<'_>,
        shutdown: bool,
    ) -> Result<bool, HttpStreamingError> {
        let result = self.poll_inner(context, shutdown);
        self.observe_drops();
        result
    }

    fn poll_inner(
        &mut self,
        context: &mut Context<'_>,
        shutdown: bool,
    ) -> Result<bool, HttpStreamingError> {
        if !self.stopping {
            match self.port.control().map(|control| control.phase()) {
                Some(crate::managed::StopPhase::Cancelled) => self.cancel_managed(),
                Some(crate::managed::StopPhase::Graceful { .. }) => self.begin_graceful(),
                _ => {}
            }
        }
        if shutdown && !self.stopping && !self.draining {
            self.cancel_with(SubscriptionTermination::ServerShutdown, None);
        }
        self.update_policy(context)?;
        self.observe_drops();
        #[cfg(test)]
        self.port.run_before_worker_for_test();
        let signal = if let Some(events) = self.events.as_mut() {
            match events.poll_worker(context) {
                Poll::Ready(signal) => Some(signal),
                Poll::Pending => None,
            }
        } else {
            None
        };
        if self
            .events
            .as_ref()
            .is_some_and(HttpSubscriptionEvents::logical_terminal_selected)
        {
            // The sole outbox owner cannot evict between the outside worker's
            // logical decision and this freeze, including pending cleanup.
            self.outbox.freeze_terminal_counter();
        }
        let candidate = match signal {
            Some(HttpSubscriptionSignal::Event {
                reservation,
                output,
            }) => {
                self.demanded = false;
                if self.stopping || (self.draining && !self.graceful_event) {
                    drop((reservation, output));
                    None
                } else {
                    match output {
                        SubscriptionOutput::Event(response) => Some((reservation, response)),
                        SubscriptionOutput::Complete => {
                            self.pending_terminal = Some((reservation, None));
                            self.finish_observation(SubscriptionTermination::SourceComplete);
                            None
                        }
                        SubscriptionOutput::Terminal(response) => {
                            let decision = response.decision();
                            self.pending_terminal = Some((reservation, Some(response)));
                            self.finish_observation(decision);
                            None
                        }
                        SubscriptionOutput::Cancelled => {
                            drop(reservation);
                            match self.port.control().map(|control| control.phase()) {
                                Some(crate::managed::StopPhase::Graceful { .. }) => {
                                    self.begin_graceful();
                                    context.waker().wake_by_ref();
                                }
                                Some(crate::managed::StopPhase::Cancelled) => self.cancel_managed(),
                                _ => self.cancel_with(SubscriptionTermination::Internal, None),
                            }
                            None
                        }
                    }
                }
            }
            Some(HttpSubscriptionSignal::Finished) => {
                self.source_done = true;
                self.events.take();
                if !self.stopping
                    && !self.draining
                    && self.pending_terminal.is_none()
                    && self.terminal.is_none()
                {
                    return Err(HttpStreamingError::Worker);
                }
                None
            }
            Some(HttpSubscriptionSignal::Cancelled) => {
                self.source_done = true;
                self.events.take();
                None
            }
            Some(HttpSubscriptionSignal::TimedOut) => {
                self.source_done = true;
                self.events.take();
                return Err(HttpStreamingError::TimedOut);
            }
            Some(HttpSubscriptionSignal::Failed) => {
                self.source_done = true;
                self.events.take();
                return Err(HttpStreamingError::Worker);
            }
            None => None,
        };
        self.stage_failure();
        self.produce(context, candidate)?;
        self.finish_graceful();
        Ok(self.source_done && self.wire_closed)
    }

    pub(super) fn cancel(&mut self, error: Option<HttpStreamingError>) {
        self.cancel_with(SubscriptionTermination::TransportClosed, error);
    }

    pub(super) fn cancel_with(
        &mut self,
        decision: SubscriptionTermination,
        error: Option<HttpStreamingError>,
    ) {
        self.finish_observation(decision);
        self.stopping = true;
        self.wire_closed = true;
        self.outbox.cancel();
        self.heartbeat.cancel();
        self.pending_terminal.take();
        self.failure.take();
        self.terminal.take();
        self.current.take();
        if let Some(events) = &mut self.events {
            events.cancel();
        }
        self.port.finish(error);
    }

    fn observe_drops(&mut self) {
        if let Some(events) = &mut self.events {
            events.observe_drops(&mut self.outbox);
        }
    }

    fn finish_observation(&mut self, decision: SubscriptionTermination) {
        let total = self.outbox.freeze_terminal_counter();
        self.observe_drops();
        if let Some(events) = &mut self.events {
            events.finish_observation(decision, total);
        }
    }

    pub(super) fn next_deadline(&mut self) -> Result<Option<u64>, HttpStreamingError> {
        if let Some(CurrentFrame::RetiredEvent(progress)) = &self.current {
            return progress
                .next_deadline()
                .map_err(HttpStreamingError::Delivery);
        }
        if self.stopping || self.terminal.is_some() {
            return Ok(None);
        }
        let policy = self
            .outbox
            .next_deadline()
            .map_err(HttpStreamingError::Delivery)?;
        if self.draining {
            return Ok(policy);
        }
        Ok(Some(policy.map_or(self.heartbeat_at, |time| {
            time.min(self.heartbeat_at)
        })))
    }
}

fn interval_ns(configuration: &HttpStreamConfiguration) -> Result<u64, HttpStreamingError> {
    u64::try_from(configuration.heartbeat.as_nanos()).map_err(|_| HttpStreamingError::Clock)
}
