//! Sole acknowledgement write and successful-flush gate.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_http::{DeadlineFactory, WebSocketClientMessage};
use chigiri_runtime::Object;

use super::super::request::AdmittedWebSocketRequest;
use super::authorization::{AuthorizationErrorObserver, AuthorizationFrameworkFailure};
use super::cleanup::{self, WebSocketConnectionCleanup};
use super::close::WebSocketConnectionClose;
use super::frame::{
    AckGate, WebSocketAcknowledgementFrame, WebSocketAcknowledgementProgress,
    WebSocketConnectionFrame,
};
use super::initialized::InitializedWebSocketConnection;
use super::input::WebSocketInitializationInput;
use super::state::WebSocketAcknowledgementResolution;
use crate::subscription::bridge_resources::BridgeConnection;
use crate::subscription::control::{WebSocketControlClass, WebSocketControlStaging};
use crate::subscription::delivery::EndpointByteOwner;

/// Accepted initialization whose acknowledgement is not yet physically flushed.
#[must_use = "the acknowledgement must be written or the connection closed"]
#[doc(hidden)]
pub struct AckPendingWebSocketConnection {
    connection: Option<BridgeConnection>,
    request: Arc<AdmittedWebSocketRequest>,
    owner: EndpointByteOwner,
    payload: Option<Arc<Object>>,
    payload_charge: usize,
    controls: Option<WebSocketControlStaging>,
    deadlines: &'static dyn DeadlineFactory,
    observer: Arc<dyn AuthorizationErrorObserver>,
    gate: Option<Arc<AckGate>>,
    terminal: Option<WebSocketConnectionClose>,
}

impl AckPendingWebSocketConnection {
    #[allow(clippy::too_many_arguments)]
    pub(super) const fn new(
        connection: BridgeConnection,
        request: Arc<AdmittedWebSocketRequest>,
        owner: EndpointByteOwner,
        payload: Option<Arc<Object>>,
        payload_charge: usize,
        controls: WebSocketControlStaging,
        deadlines: &'static dyn DeadlineFactory,
        observer: Arc<dyn AuthorizationErrorObserver>,
    ) -> Self {
        Self {
            connection: Some(connection),
            request,
            owner,
            payload,
            payload_charge,
            controls: Some(controls),
            deadlines,
            observer,
            gate: None,
            terminal: None,
        }
    }

    /// Begins the sole priority acknowledgement without copying encoded bytes.
    pub fn begin_acknowledgement(
        &mut self,
    ) -> Result<
        Option<(
            WebSocketAcknowledgementFrame,
            WebSocketAcknowledgementProgress,
        )>,
        WebSocketConnectionClose,
    > {
        if let Some(close) = self.terminal {
            return Err(close);
        }
        if self.gate.is_some() {
            return Ok(None);
        }
        let frame = self
            .controls
            .as_mut()
            .expect("live acknowledgement controls")
            .begin_send_for(WebSocketControlClass::Priority)
            .map_err(|_| self.fail_framework())?;
        let Some(frame) = frame else {
            return Ok(None);
        };
        let physical = self
            .connection
            .as_ref()
            .expect("live acknowledgement connection")
            .physical_lease()
            .map_err(|_| self.fail_framework())?;
        let gate = Arc::new(AckGate::default());
        self.gate = Some(Arc::clone(&gate));
        Ok(Some((
            WebSocketAcknowledgementFrame::new(
                WebSocketConnectionFrame::new(frame, physical),
                Arc::clone(&gate),
            ),
            WebSocketAcknowledgementProgress::new(gate),
        )))
    }

    /// Classifies a pre-flush message without admitting an operation.
    pub fn receive_text(
        &mut self,
        text: &str,
    ) -> Result<WebSocketInitializationInput, WebSocketConnectionClose> {
        let message = chigiri_http::__decode_websocket_client_message(text)
            .map_err(|_| self.terminate(WebSocketConnectionClose::BAD_REQUEST))?;
        match message {
            WebSocketClientMessage::ConnectionInit(_) => {
                Err(self.terminate(WebSocketConnectionClose::TOO_MANY_INIT))
            }
            WebSocketClientMessage::Subscribe(_) => {
                Err(self.terminate(WebSocketConnectionClose::UNAUTHORIZED))
            }
            WebSocketClientMessage::Ping(payload) => {
                self.controls
                    .as_mut()
                    .expect("live acknowledgement controls")
                    .enqueue_peer_pong(&payload)
                    .map_err(|_| self.fail_framework())?;
                Ok(WebSocketInitializationInput::ControlQueued)
            }
            WebSocketClientMessage::Pong(_) => Ok(WebSocketInitializationInput::PongObserved),
            WebSocketClientMessage::Complete(_) => Ok(WebSocketInitializationInput::Ignored),
        }
    }

    /// Rejects one binary message before acknowledgement completes.
    pub fn receive_binary(&mut self) -> WebSocketConnectionClose {
        self.terminate(WebSocketConnectionClose::BAD_REQUEST)
    }

    pub(crate) fn request_close(
        &mut self,
        close: WebSocketConnectionClose,
    ) -> WebSocketConnectionClose {
        self.terminate(close)
    }

    fn terminate(&mut self, close: WebSocketConnectionClose) -> WebSocketConnectionClose {
        self.terminal.get_or_insert(close);
        close
    }

    fn fail_framework(&mut self) -> WebSocketConnectionClose {
        self.observer
            .observe(&AuthorizationFrameworkFailure::Control);
        self.terminate(WebSocketConnectionClose::INTERNAL)
    }

    fn cleanup(&mut self, close: WebSocketConnectionClose) -> WebSocketConnectionCleanup {
        cleanup::framework(
            self.connection
                .take()
                .expect("live acknowledgement connection"),
            cleanup::disposal::ConnectionDisposal {
                _payload: self.payload.take(),
                _controls: self.controls.take(),
                _acknowledgement: self.gate.take(),
                ..cleanup::disposal::ConnectionDisposal::metadata(
                    Arc::clone(&self.request),
                    self.owner.clone(),
                )
            },
            close,
            self.deadlines,
        )
    }

    fn initialized(&mut self) -> InitializedWebSocketConnection {
        InitializedWebSocketConnection::new(
            self.connection.take().expect("acknowledged connection"),
            Arc::clone(&self.request),
            self.owner.clone(),
            self.payload.take(),
            self.payload_charge,
            self.controls.take().expect("acknowledged controls"),
            self.deadlines,
            Arc::clone(&self.observer),
        )
    }
}

impl Future for AckPendingWebSocketConnection {
    type Output = WebSocketAcknowledgementResolution;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(close) = self.terminal {
            return Poll::Ready(WebSocketAcknowledgementResolution::Closing(
                self.cleanup(close),
            ));
        }
        let Some(gate) = &self.gate else {
            return Poll::Pending;
        };
        match gate.poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(WebSocketAcknowledgementResolution::Initialized(
                self.initialized(),
            )),
            Poll::Ready(Err(())) => Poll::Ready(WebSocketAcknowledgementResolution::Closing(
                self.cleanup(WebSocketConnectionClose::INTERNAL),
            )),
        }
    }
}

impl Drop for AckPendingWebSocketConnection {
    fn drop(&mut self) {
        if self.connection.is_some() {
            drop(self.cleanup(WebSocketConnectionClose::INTERNAL));
        }
    }
}
