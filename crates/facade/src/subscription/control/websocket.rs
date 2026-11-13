//! Fixed non-evicting WebSocket controls with class-preserving selection.

mod accounting;
mod operation;
mod schedule;

pub(super) use accounting::QueueAccounting;
#[cfg(test)]
pub(super) use accounting::{MAX_QUEUED_CONTROL_BYTES, MAX_QUEUED_CONTROL_MESSAGES};
pub(crate) use operation::WebSocketControlOperation;
pub use schedule::{WebSocketControlClass, WebSocketWriterSchedule, WebSocketWriterTurn};

use std::collections::VecDeque;
use std::sync::Arc;

use chigiri_http::{
    __encode_subscription_control, __encode_websocket_peer_pong, __subscription_static_control_len,
    __websocket_peer_pong_len, SubscriptionControl, SubscriptionStaticControl,
    SubscriptionTransport, WebSocketControlPayload,
};

use super::super::delivery::EndpointByteOwner;
use super::encoding::{StagedControl, encode_reserved};
use super::frame::InFlightGate;
use super::{SubscriptionControlFrame, SubscriptionControlSignal};

const MAX_CONTROL_BYTES: usize = 64 * 1024;

struct QueuedControl {
    frame: StagedControl,
    class: WebSocketControlClass,
    operation: Option<WebSocketControlOperation>,
}

/// A fixed, non-evicting WebSocket control queue plus one in-flight frame.
pub struct WebSocketControlStaging {
    bytes: EndpointByteOwner,
    queue: VecDeque<QueuedControl>,
    accounting: QueueAccounting,
    in_flight: Arc<InFlightGate>,
    terminal: Option<SubscriptionControlSignal>,
}

impl WebSocketControlStaging {
    pub(crate) fn new(bytes: EndpointByteOwner) -> Self {
        Self {
            bytes,
            queue: VecDeque::new(),
            accounting: QueueAccounting::default(),
            in_flight: Arc::new(InFlightGate::default()),
            terminal: None,
        }
    }

    /// Queues one supported static WebSocket control after exact reservation.
    pub fn enqueue_static(
        &mut self,
        control: SubscriptionStaticControl,
    ) -> Result<(), SubscriptionControlSignal> {
        self.ensure_live()?;
        let length = __subscription_static_control_len(SubscriptionTransport::WebSocket, control)
            .map_err(|error| self.fail(SubscriptionControlSignal::from_encoding(error)))?;
        let class = match control {
            SubscriptionStaticControl::Heartbeat => WebSocketControlClass::Ordinary,
            _ => WebSocketControlClass::Priority,
        };
        self.enqueue_exact(class, length, |maximum| {
            __encode_subscription_control(
                SubscriptionTransport::WebSocket,
                static_control(control),
                maximum,
            )
        })
    }

    /// Queues one byte-exact response to a decoder-owned peer ping.
    pub fn enqueue_peer_pong(
        &mut self,
        payload: &WebSocketControlPayload,
    ) -> Result<(), SubscriptionControlSignal> {
        self.ensure_live()?;
        let length = __websocket_peer_pong_len(payload);
        self.enqueue_exact(WebSocketControlClass::Ordinary, length, |maximum| {
            __encode_websocket_peer_pong(payload, maximum)
        })
    }

    /// Moves the FIFO head into at-most-one physical send ownership.
    pub fn begin_send(
        &mut self,
    ) -> Result<Option<SubscriptionControlFrame>, SubscriptionControlSignal> {
        self.ensure_live()?;
        if self.queue.is_empty() {
            return Ok(None);
        }
        self.take_at(0)
    }

    /// Reports queued work of one class without inspecting encoded bytes.
    pub fn has_queued(&self, class: WebSocketControlClass) -> bool {
        self.queue.iter().any(|control| control.class == class)
    }

    /// Takes the oldest control of a class, preserving every skipped entry.
    ///
    /// Priority controls may bypass ordinary ping/pong work, never another
    /// priority control. The same physical gate still permits only one frame.
    pub fn begin_send_for(
        &mut self,
        class: WebSocketControlClass,
    ) -> Result<Option<SubscriptionControlFrame>, SubscriptionControlSignal> {
        self.ensure_live()?;
        let Some(index) = self.queue.iter().position(|control| control.class == class) else {
            return Ok(None);
        };
        self.take_at(index)
    }

    fn take_at(
        &mut self,
        index: usize,
    ) -> Result<Option<SubscriptionControlFrame>, SubscriptionControlSignal> {
        let Some(lease) = self.in_flight.try_acquire() else {
            return Ok(None);
        };
        let Some(QueuedControl {
            frame: staged,
            operation,
            ..
        }) = self.queue.remove(index)
        else {
            return Err(self.fail(SubscriptionControlSignal::Accounting));
        };
        let length = staged.len();
        self.accounting
            .remove(length)
            .map_err(|signal| self.fail(signal))?;
        let (encoded, charge) = staged.into_parts();
        let charge = charge.into_in_flight();
        if charge.bytes() != encoded.len() {
            return Err(self.fail(SubscriptionControlSignal::Accounting));
        }
        Ok(Some(
            SubscriptionControlFrame::new(encoded, charge, lease)
                .with_websocket_operation(operation),
        ))
    }

    /// Returns the number of wholly queued controls, excluding in-flight data.
    pub const fn queued_messages(&self) -> usize {
        self.accounting.messages
    }

    /// Returns wholly queued control bytes, excluding in-flight data.
    pub const fn queued_bytes(&self) -> usize {
        self.accounting.bytes
    }

    /// Drops unstarted control backlog while allowing eligible finite work to
    /// stage its result later. The current immutable frame is unaffected.
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn discard_waiting(&mut self) {
        self.queue.clear();
        self.accounting = QueueAccounting::default();
    }

    /// Cancels staging and releases queued controls without touching in-flight owners.
    pub fn cancel(&mut self) {
        self.terminal
            .get_or_insert(SubscriptionControlSignal::Cancelled);
        self.queue.clear();
        self.accounting = QueueAccounting::default();
    }

    fn enqueue_exact<F>(
        &mut self,
        class: WebSocketControlClass,
        exact_bytes: usize,
        encode: F,
    ) -> Result<(), SubscriptionControlSignal>
    where
        F: FnOnce(
            usize,
        ) -> Result<
            chigiri_http::EncodedSubscriptionControl,
            chigiri_http::SubscriptionControlEncodingError,
        >,
    {
        self.accounting
            .exact_candidate(exact_bytes)
            .map_err(|signal| self.fail(signal))?;
        let staged = encode_reserved(&self.bytes, exact_bytes, Some(exact_bytes), encode)
            .map_err(|signal| self.fail(signal))?;
        self.push(staged, class, None)
    }

    fn enqueue_dynamic<F>(
        &mut self,
        class: WebSocketControlClass,
        operation: Option<WebSocketControlOperation>,
        encode: F,
    ) -> Result<(), SubscriptionControlSignal>
    where
        F: FnOnce(
            usize,
        ) -> Result<
            chigiri_http::EncodedSubscriptionControl,
            chigiri_http::SubscriptionControlEncodingError,
        >,
    {
        self.ensure_live()?;
        let maximum = self
            .accounting
            .dynamic_candidate()
            .map_err(|signal| self.fail(signal))?
            .min(MAX_CONTROL_BYTES);
        let staged = encode_reserved(&self.bytes, maximum, None, encode)
            .map_err(|signal| self.fail(signal))?;
        self.push(staged, class, operation)
    }

    fn push(
        &mut self,
        staged: StagedControl,
        class: WebSocketControlClass,
        operation: Option<WebSocketControlOperation>,
    ) -> Result<(), SubscriptionControlSignal> {
        self.accounting
            .admit(staged.len())
            .map_err(|signal| self.fail(signal))?;
        self.queue.push_back(QueuedControl {
            frame: staged,
            class,
            operation,
        });
        Ok(())
    }

    fn ensure_live(&self) -> Result<(), SubscriptionControlSignal> {
        self.terminal.map_or(Ok(()), Err)
    }

    fn fail(&mut self, signal: SubscriptionControlSignal) -> SubscriptionControlSignal {
        *self.terminal.get_or_insert(signal)
    }
}

fn static_control(control: SubscriptionStaticControl) -> SubscriptionControl<'static> {
    match control {
        SubscriptionStaticControl::ConnectionAck => SubscriptionControl::ConnectionAck,
        SubscriptionStaticControl::Heartbeat => SubscriptionControl::Heartbeat,
        SubscriptionStaticControl::Complete => SubscriptionControl::Complete {
            websocket_operation_id: None,
        },
    }
}
