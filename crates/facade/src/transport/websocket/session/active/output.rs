//! Sole fair whole-frame writer with a final same-timeline generation fence.

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::SubscriptionStaticControl;

use super::super::operation::WebSocketOperationSharedFailure;
use super::super::writer::OpeningWrite;
use super::super::{WebSocketIo, WebSocketWireFrame};
use super::graceful::SendAuthorization;
use super::{ActiveConnection, CurrentWrite};
use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::control::{WebSocketControlClass, WebSocketWriterTurn};
use crate::transport::websocket::WebSocketConnectionFrame;

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    pub(super) fn poll_writer(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        self.sync_shutdown();
        if self.closing.is_some() {
            return Ok(true);
        }
        if let Some(current) = self.current.take() {
            return self.poll_current(current, context, now);
        }
        match Pin::new(self.socket.as_mut().expect("live socket")).poll_ready(context) {
            Poll::Pending => return Ok(false),
            Poll::Ready(Err(error)) => {
                self.io_failure(error);
                return Ok(true);
            }
            Poll::Ready(Ok(())) => {}
        }
        // Native readiness may race the endpoint signal. Recheck before a new
        // whole-frame transfer; never hold SDK locks across native I/O polls.
        self.sync_shutdown();
        if self.closing.is_some() {
            return Ok(true);
        }
        let parts = self.parts.as_mut().expect("live initialized connection");
        if self.heartbeat_due {
            parts
                .controls
                .enqueue_static(SubscriptionStaticControl::Heartbeat)?;
            self.heartbeat_due = false;
        }
        for (ready, slot) in self.ready.iter_mut().zip(&self.slots) {
            *ready = slot.as_ref().is_some_and(|slot| slot.has_event());
        }
        let turn = self.schedule.select(
            &self.ready,
            parts.controls.has_queued(WebSocketControlClass::Priority),
            parts.controls.has_queued(WebSocketControlClass::Ordinary),
        );
        match turn {
            None => Ok(false),
            Some(WebSocketWriterTurn::Control(class)) => {
                let Some(frame) = parts.controls.begin_send_for(class)? else {
                    return Ok(false);
                };
                if let Some(generation) = frame.websocket_generation() {
                    let id = frame.websocket_operation_id().ok_or_else(invariant)?;
                    if self.slots.iter().flatten().any(|slot| {
                        slot.wire_id() == Some(id)
                            && slot.live_generation().is_some()
                            && slot.generation() != generation
                    }) {
                        // Suppress a stale tagged permission before the sole
                        // start_send fence even if its old slot was reaped.
                        return Ok(true);
                    }
                    if let Some(slot) = self
                        .slots
                        .iter_mut()
                        .flatten()
                        .find(|slot| slot.generation() == generation)
                        && !slot.authorize_terminal_control(&frame)
                    {
                        return Ok(true);
                    }
                    // A reaped slot already transferred its complete terminal
                    // permission to this queue. New admission/complete removes
                    // that exact queued permission before start_send can run.
                }
                let physical = parts.connection.physical_lease()?;
                let frame =
                    WebSocketWireFrame::control(WebSocketConnectionFrame::new(frame, physical));
                let timeout = self
                    .configuration
                    .execution
                    .subscriptions
                    .limits()
                    .send_no_progress_timeout;
                let mut write = OpeningWrite::begin(frame, None, parts.deadlines, timeout);
                if let Err(error) = write.check_start(context) {
                    self.io_failure(error);
                    return Ok(true);
                }
                #[cfg(test)]
                self.before_send_for_test();
                let authorization = self.authorize_send(false);
                if !matches!(authorization, SendAuthorization::Allowed) {
                    drop(write);
                    self.reconcile_send(authorization);
                    return Ok(true);
                }
                if let Err(error) =
                    write.start_authorized(self.socket.as_mut().expect("live socket"))
                {
                    self.io_failure(error);
                    return Ok(true);
                }
                self.current = Some(CurrentWrite::Control(write));
                self.schedule.control_acquired(class);
                Ok(true)
            }
            Some(WebSocketWriterTurn::Event(index)) => {
                let slot = self.slots[index].as_ref().ok_or_else(invariant)?;
                let generation = slot.generation();
                let id = slot.wire_id().ok_or_else(invariant)?;
                if self.slots.iter().flatten().any(|other| {
                    other.generation() != generation
                        && other.wire_id() == Some(id)
                        && other.live_generation().is_some()
                }) {
                    return Err(invariant());
                }
                let Some(frame) = self.slots[index]
                    .as_mut()
                    .expect("ready operation")
                    .begin_event(now)?
                else {
                    return Ok(false);
                };
                #[cfg(test)]
                self.before_send_for_test();
                let finite = self.slots[index]
                    .as_ref()
                    .expect("ready operation")
                    .is_finite();
                let authorization = self.authorize_send(finite);
                if !matches!(authorization, SendAuthorization::Allowed) {
                    // No native owner ever received this frame. Clear only its
                    // new progress before cancellation, so cleanup cannot wait
                    // for an impossible flush or bypass this skipped result.
                    drop(frame);
                    self.slots[index]
                        .as_mut()
                        .expect("ready operation")
                        .discard_unstarted_event(
                            &mut self.parts.as_mut().expect("live controls").controls,
                        )?;
                    self.reconcile_send(authorization);
                    return Ok(true);
                }
                // Readiness, the generation fence, acquisition and start_send
                // share this exclusive synchronous interval with no input poll.
                if let Err(error) =
                    Pin::new(self.socket.as_mut().expect("live socket")).start_send(frame)
                {
                    self.io_failure(error);
                    return Ok(true);
                }
                self.current = Some(CurrentWrite::Event {
                    slot: index,
                    generation,
                    flushed: false,
                });
                self.schedule.event_acquired(index, self.slots.len())?;
                Ok(true)
            }
        }
    }

    fn poll_current(
        &mut self,
        mut current: CurrentWrite,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        let done = match &mut current {
            CurrentWrite::Control(write) => {
                let parts = self.parts.as_ref().expect("live connection");
                let timeout = self
                    .configuration
                    .execution
                    .subscriptions
                    .limits()
                    .send_no_progress_timeout;
                match write.poll(
                    self.socket.as_mut().expect("live socket"),
                    context,
                    parts.deadlines,
                    timeout,
                ) {
                    Poll::Pending => false,
                    Poll::Ready(Ok(done)) => {
                        if !done {
                            self.current = Some(current);
                        }
                        return Ok(true);
                    }
                    Poll::Ready(Err(error)) => {
                        self.io_failure(error);
                        return Ok(true);
                    }
                }
            }
            CurrentWrite::Event {
                slot: index,
                generation,
                flushed,
            } => {
                let slot = self.slots[*index].as_mut().ok_or_else(invariant)?;
                if slot.generation() != *generation {
                    return Err(invariant());
                }
                if !*flushed {
                    match Pin::new(self.socket.as_mut().expect("live socket")).poll_write(context) {
                        Poll::Pending => {}
                        Poll::Ready(Err(error)) => {
                            self.io_failure(error);
                            return Ok(true);
                        }
                        Poll::Ready(Ok(progress)) => {
                            slot.record_event_progress(progress.accepted, progress.flushed, now)?;
                            *flushed = progress.flushed;
                            if !*flushed {
                                self.current = Some(current);
                                return Ok(true);
                            }
                        }
                    }
                }
                if *flushed {
                    match slot.poll_finish_event(context, now) {
                        Poll::Ready(Ok(true)) => true,
                        Poll::Ready(Ok(false)) | Poll::Pending => false,
                        Poll::Ready(Err(error)) => return Err(error),
                    }
                } else {
                    false
                }
            }
        };
        if !done {
            self.current = Some(current);
        }
        Ok(done)
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Resource(BridgeResourceError::Invariant)
}
