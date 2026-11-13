//! Bounded fair operation sweeps and independent connection shutdown.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use super::super::WebSocketIo;
use super::super::closing::Closing;
use super::super::operation::WebSocketOperationSharedFailure;
use super::ActiveConnection;
use crate::transport::websocket::WebSocketConnectionClose;

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    fn poll_slots(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        let mut progressed = false;
        for index in 0..self.slots.len() {
            // Each transition is closed internal work; no application callback
            // runs on this connection task. Bound every per-slot sweep.
            for _ in 0..8 {
                let Some(slot) = &mut self.slots[index] else {
                    break;
                };
                let controls = match &mut self.parts {
                    Some(parts) => &mut parts.controls,
                    None => self.closing_controls.as_mut().expect("closing controls"),
                };
                if self.draining {
                    slot.begin_graceful(controls)?;
                }
                let transition = match slot.poll(context, now, controls) {
                    Poll::Pending => break,
                    Poll::Ready(Err(error)) => return Err(error),
                    Poll::Ready(Ok(transition)) => transition,
                };
                self.consume_transition(index, transition);
                progressed = true;
            }
        }
        Ok(progressed)
    }

    fn arm_timers(
        &mut self,
        context: &mut Context<'_>,
        now: u64,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        let parts = self.parts.as_ref().expect("live connection timers");
        let mut progressed = false;
        if !self.draining && self.heartbeat.as_mut().poll(context).is_ready() {
            self.heartbeat_due = true;
            self.heartbeat = parts.deadlines.sleep(
                self.configuration
                    .execution
                    .subscriptions
                    .limits()
                    .heartbeat_interval,
            );
            progressed = true;
        }
        let mut earliest = None;
        for slot in self.slots.iter_mut().flatten() {
            if let Some(at) = slot.next_deadline()? {
                earliest = Some(earliest.map_or(at, |old: u64| old.min(at)));
            }
        }
        if self.event_timer.as_ref().map(|(at, _)| *at) != earliest {
            self.event_timer = earliest.map(|at| {
                (
                    at,
                    parts
                        .deadlines
                        .sleep(Duration::from_nanos(at.saturating_sub(now))),
                )
            });
        }
        if let Some((_, timer)) = &mut self.event_timer {
            progressed |= timer.as_mut().poll(context).is_ready();
        }
        Ok(progressed)
    }

    pub(super) fn close(&mut self, close: WebSocketConnectionClose, send: bool) {
        self.close_with(
            close,
            send,
            chigiri_runtime::__private::SubscriptionTermination::TransportClosed,
        );
    }

    pub(super) fn close_internal(&mut self) {
        self.close_with(
            WebSocketConnectionClose::internal(),
            false,
            chigiri_runtime::__private::SubscriptionTermination::Internal,
        );
    }

    fn close_failure(&mut self, error: WebSocketOperationSharedFailure) {
        use chigiri_runtime::__private::SubscriptionTermination;
        let decision = match error {
            // The initiating operation has already selected DeliveryFailed;
            // only its siblings now lose the shared physical transport.
            WebSocketOperationSharedFailure::Delivery(
                crate::subscription::outbox::SubscriptionOutboxSignal::SendStalled,
            ) => SubscriptionTermination::TransportClosed,
            WebSocketOperationSharedFailure::Control(signal) => {
                crate::subscription::failure::control_termination(signal)
            }
            _ => SubscriptionTermination::Internal,
        };
        self.close_with(WebSocketConnectionClose::internal(), false, decision);
    }

    pub(in crate::transport::websocket::session) fn shutdown(&mut self) {
        self.close_with(
            WebSocketConnectionClose::internal(),
            false,
            chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
        );
    }

    pub(in crate::transport::websocket::session) fn transport_closed(&mut self) {
        self.close(WebSocketConnectionClose::internal(), false);
    }

    fn close_with(
        &mut self,
        close: WebSocketConnectionClose,
        send: bool,
        decision: chigiri_runtime::__private::SubscriptionTermination,
    ) {
        let Some(mut parts) = self.parts.take() else {
            return;
        };
        parts.request_close(close);
        for slot in self.slots.iter_mut().flatten() {
            // Any invariant failure is already a whole-connection close. Drop
            // still preserves the concrete worker's original quarantine owner.
            let _ = slot.disconnect_with(&mut parts.controls, decision);
        }
        self.current.take();
        self.event_timer.take();
        let deadlines = parts.deadlines;
        let (cleanup, controls) = parts.into_shutdown();
        self.closing = Some(Closing::new(cleanup, deadlines, send));
        self.closing_controls = Some(controls);
    }

    fn poll_closing(&mut self, context: &mut Context<'_>, now: u64) -> Poll<()> {
        // Failure cannot revive output; dropping a broken slot transfers its
        // remaining original concrete cleanup owner to charged quarantine.
        if self.poll_slots(context, now).is_err() {
            self.slots.iter_mut().for_each(|slot| {
                slot.take();
            });
        }
        let operations_done = self.slots.iter().all(Option::is_none);
        let closing = self.closing.as_mut().expect("closing connection");
        if let Some(socket) = self.socket.as_mut() {
            if closing.poll_socket(socket, context).is_pending() {
                return Poll::Pending;
            }
            self.socket.take();
        }
        let result = closing.poll_cleanup(context, operations_done);
        if result.is_ready() {
            // Destroy the real upstream buffers before any remaining physical
            // operation metadata or connection-cleanup owner is released.
            self.socket.take();
            self.slots.iter_mut().for_each(|slot| {
                slot.take();
            });
            self.closing.take();
            self.closing_controls.take();
        }
        result
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> Future for ActiveConnection<C, S> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        for _ in 0..32 {
            if self.socket.is_none() && self.closing.is_none() {
                return Poll::Ready(());
            }
            let now = match self.clock.now() {
                Some(now) => now,
                None => {
                    // Overflow is a failed accounting clock, not a synthetic
                    // timestamp that can silently expire or revive output.
                    self.close_internal();
                    return self.poll_closing(context, 0);
                }
            };
            if self.closing.is_some() {
                return self.poll_closing(context, now);
            }
            self.sync_shutdown();
            if self.closing.is_some() {
                return self.poll_closing(context, now);
            }
            let result = (|| {
                let operations = self.poll_slots(context, now)?;
                let timers = self.arm_timers(context, now)?;
                let input = if self.draining {
                    false
                } else {
                    self.poll_input(context)?
                };
                if self.closing.is_some() {
                    return Ok(true);
                }
                let output = self.poll_writer(context, now)?;
                self.finish_graceful();
                Ok::<_, WebSocketOperationSharedFailure>(operations || timers || input || output)
            })();
            match result {
                Ok(false) => return Poll::Pending,
                Ok(true) => {}
                Err(error) => self.close_failure(error),
            }
        }
        context.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<C: Send + Sync + 'static, S: WebSocketIo> Drop for ActiveConnection<C, S> {
    fn drop(&mut self) {
        self.close_internal();
        self.socket.take();
        self.slots.iter_mut().for_each(|slot| {
            slot.take();
        });
        self.closing.take();
    }
}
