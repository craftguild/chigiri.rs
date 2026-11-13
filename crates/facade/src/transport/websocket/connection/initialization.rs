//! Externally timed authorization before acknowledgement can be written.

mod worker;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_http::{
    ConnectionInitAuthorizer, ConnectionInitDecision, DeadlineFactory, SubscriptionStaticControl,
    WebSocketClientMessage,
};
use chigiri_runtime::Object;
use chigiri_runtime::core::executor::SubscriptionMetadataCharge;

use super::super::request::AdmittedWebSocketRequest;
use super::acknowledgement::AckPendingWebSocketConnection;
use super::authorization::{
    AuthorizationErrorObserver, AuthorizationFrameworkFailure, AuthorizationResult,
};
use super::cleanup::{self, WebSocketConnectionCleanup};
use super::close::WebSocketConnectionClose;
use super::frame::WebSocketConnectionFrame;
use super::input::WebSocketInitializationInput;
use super::state::{WebSocketConnectionResolution, begin_control};
use crate::subscription::bridge_resources::BridgeConnection;
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::worker::{ConcreteWorker, ConcreteWorkerOutcome};

/// Application-polled initialization before any GraphQL operation is admitted.
#[must_use = "connection initialization must be polled or dropped into cleanup"]
#[doc(hidden)]
pub struct WebSocketConnectionInitialization {
    connection: Option<BridgeConnection>,
    request: Arc<AdmittedWebSocketRequest>,
    owner: EndpointByteOwner,
    payload: Option<Arc<Object>>,
    payload_charge: usize,
    controls: Option<WebSocketControlStaging>,
    timer: Option<chigiri_http::BoxFuture<'static, ()>>,
    deadlines: &'static dyn DeadlineFactory,
    authorizer: Arc<dyn ConnectionInitAuthorizer>,
    observer: Arc<dyn AuthorizationErrorObserver>,
    activity: crate::managed::EventActivity,
    worker: Option<ConcreteWorker<AuthorizationResult>>,
    init_received: bool,
    terminal: Option<WebSocketConnectionClose>,
}

impl WebSocketConnectionInitialization {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        connection: BridgeConnection,
        request: Arc<AdmittedWebSocketRequest>,
        owner: EndpointByteOwner,
        controls: WebSocketControlStaging,
        timer: chigiri_http::BoxFuture<'static, ()>,
        deadlines: &'static dyn DeadlineFactory,
        authorizer: Arc<dyn ConnectionInitAuthorizer>,
        observer: Arc<dyn AuthorizationErrorObserver>,
        activity: crate::managed::EventActivity,
    ) -> Self {
        Self {
            connection: Some(connection),
            request,
            owner,
            payload: None,
            payload_charge: 0,
            controls: Some(controls),
            timer: Some(timer),
            deadlines,
            authorizer,
            observer,
            activity,
            worker: None,
            init_received: false,
            terminal: None,
        }
    }

    /// Accepts one complete UTF-8 protocol message without admitting operations.
    pub fn receive_text(
        &mut self,
        text: &str,
    ) -> Result<WebSocketInitializationInput, WebSocketConnectionClose> {
        if let Some(close) = self.terminal {
            return Err(close);
        }
        let message = chigiri_http::__decode_websocket_client_message(text)
            .map_err(|_| self.terminate(WebSocketConnectionClose::BAD_REQUEST))?;
        self.receive_message(message)
    }

    /// Rejects one binary GraphQL protocol message with the frozen close.
    pub fn receive_binary(&mut self) -> WebSocketConnectionClose {
        self.terminate(WebSocketConnectionClose::BAD_REQUEST)
    }

    pub(crate) fn request_close(
        &mut self,
        close: WebSocketConnectionClose,
    ) -> WebSocketConnectionClose {
        self.terminate(close)
    }

    /// Begins one staged pre-ack ping response with physical quota held.
    pub fn begin_control_send(
        &mut self,
    ) -> Result<Option<WebSocketConnectionFrame>, WebSocketConnectionClose> {
        begin_control(
            self.connection
                .as_ref()
                .expect("live initialization connection"),
            self.controls
                .as_mut()
                .expect("live initialization controls"),
        )
        .map_err(|()| self.fail_framework(AuthorizationFrameworkFailure::Control))
    }

    fn receive_message(
        &mut self,
        message: WebSocketClientMessage,
    ) -> Result<WebSocketInitializationInput, WebSocketConnectionClose> {
        match message {
            WebSocketClientMessage::ConnectionInit(payload) if !self.init_received => {
                self.init_received = true;
                self.payload = payload.into_arc();
                self.payload_charge = charge_payload(self.payload.as_deref())
                    .ok_or_else(|| self.terminate(WebSocketConnectionClose::BAD_REQUEST))?;
                self.start_authorization()?;
                Ok(WebSocketInitializationInput::AuthorizationStarted)
            }
            WebSocketClientMessage::ConnectionInit(_) => {
                Err(self.terminate(WebSocketConnectionClose::TOO_MANY_INIT))
            }
            WebSocketClientMessage::Subscribe(_) => {
                Err(self.terminate(WebSocketConnectionClose::UNAUTHORIZED))
            }
            WebSocketClientMessage::Ping(payload) => {
                self.controls
                    .as_mut()
                    .expect("live initialization controls")
                    .enqueue_peer_pong(&payload)
                    .map_err(|_| self.fail_framework(AuthorizationFrameworkFailure::Control))?;
                Ok(WebSocketInitializationInput::ControlQueued)
            }
            WebSocketClientMessage::Pong(_) => Ok(WebSocketInitializationInput::PongObserved),
            WebSocketClientMessage::Complete(_) => Ok(WebSocketInitializationInput::Ignored),
        }
    }

    fn terminate(&mut self, close: WebSocketConnectionClose) -> WebSocketConnectionClose {
        self.terminal.get_or_insert(close);
        close
    }

    fn fail_framework(
        &mut self,
        failure: AuthorizationFrameworkFailure,
    ) -> WebSocketConnectionClose {
        self.observer.observe(&failure);
        self.terminate(WebSocketConnectionClose::INTERNAL)
    }

    fn finish_inline(&mut self, close: WebSocketConnectionClose) -> WebSocketConnectionCleanup {
        cleanup::framework(
            self.connection
                .take()
                .expect("live initialization connection"),
            cleanup::disposal::ConnectionDisposal {
                _payload: self.payload.take(),
                _timers: [self.timer.take(), None],
                _controls: self.controls.take(),
                ..cleanup::disposal::ConnectionDisposal::metadata(
                    Arc::clone(&self.request),
                    self.owner.clone(),
                )
            },
            close,
            self.deadlines,
        )
    }

    fn finish_worker(
        &mut self,
        close: WebSocketConnectionClose,
        worker: ConcreteWorker<AuthorizationResult>,
    ) -> WebSocketConnectionCleanup {
        drop(self.payload.take());
        drop(self.controls.take());
        cleanup::worker(
            self.connection
                .take()
                .expect("live initialization connection"),
            worker,
            self.timer.take().expect("live initialization timer"),
            close,
            self.deadlines,
        )
    }

    fn accept(&mut self) -> WebSocketConnectionResolution {
        if self
            .controls
            .as_mut()
            .expect("live initialization controls")
            .enqueue_static(SubscriptionStaticControl::ConnectionAck)
            .is_err()
        {
            let close = self.fail_framework(AuthorizationFrameworkFailure::Control);
            return WebSocketConnectionResolution::Closing(self.finish_inline(close));
        }
        drop(self.timer.take());
        WebSocketConnectionResolution::Acknowledgement(AckPendingWebSocketConnection::new(
            self.connection.take().expect("accepted connection"),
            Arc::clone(&self.request),
            self.owner.clone(),
            self.payload.take(),
            self.payload_charge,
            self.controls.take().expect("accepted controls"),
            self.deadlines,
            Arc::clone(&self.observer),
        ))
    }
}

impl Future for WebSocketConnectionInitialization {
    type Output = WebSocketConnectionResolution;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(close) = self.terminal {
            let cleanup = if let Some(worker) = self.worker.take() {
                self.finish_worker(close, worker)
            } else {
                self.finish_inline(close)
            };
            return Poll::Ready(WebSocketConnectionResolution::Closing(cleanup));
        }
        if self
            .timer
            .as_mut()
            .expect("live timer")
            .as_mut()
            .poll(context)
            .is_ready()
        {
            let close = self.terminate(WebSocketConnectionClose::TIMED_OUT);
            let cleanup = if let Some(worker) = self.worker.take() {
                self.finish_worker(close, worker)
            } else {
                self.finish_inline(close)
            };
            return Poll::Ready(WebSocketConnectionResolution::Closing(cleanup));
        }
        let Some(worker) = &mut self.worker else {
            return Poll::Pending;
        };
        let outcome = match Pin::new(worker).poll(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        let worker = self.worker.take().expect("completed authorization worker");
        let resolution = match outcome {
            ConcreteWorkerOutcome::Completed(Ok(Some(ConnectionInitDecision::Accept))) => {
                drop(worker);
                self.accept()
            }
            ConcreteWorkerOutcome::Completed(Ok(Some(ConnectionInitDecision::Reject))) => {
                drop(worker);
                WebSocketConnectionResolution::Closing(
                    self.finish_inline(WebSocketConnectionClose::FORBIDDEN),
                )
            }
            ConcreteWorkerOutcome::Completed(Ok(None)) => {
                drop(worker);
                WebSocketConnectionResolution::Closing(
                    self.finish_inline(WebSocketConnectionClose::INTERNAL),
                )
            }
            ConcreteWorkerOutcome::Completed(Err(
                crate::subscription::worker::SubscriptionWorkerRuntimeError::EndpointClosed,
            )) => {
                drop(worker);
                WebSocketConnectionResolution::Closing(
                    self.finish_inline(WebSocketConnectionClose::INTERNAL),
                )
            }
            ConcreteWorkerOutcome::Completed(Err(_)) => {
                drop(worker);
                let close = self.fail_framework(AuthorizationFrameworkFailure::Panicked);
                WebSocketConnectionResolution::Closing(self.finish_inline(close))
            }
            ConcreteWorkerOutcome::Cancelled => {
                drop(worker);
                let close = self.fail_framework(AuthorizationFrameworkFailure::Cancelled);
                WebSocketConnectionResolution::Closing(self.finish_inline(close))
            }
            ConcreteWorkerOutcome::Panicked => {
                drop(worker);
                let close = self.fail_framework(AuthorizationFrameworkFailure::Panicked);
                WebSocketConnectionResolution::Closing(self.finish_inline(close))
            }
            ConcreteWorkerOutcome::Poisoned => {
                let close = self.fail_framework(AuthorizationFrameworkFailure::Poisoned);
                WebSocketConnectionResolution::Closing(self.finish_worker(close, worker))
            }
        };
        Poll::Ready(resolution)
    }
}

impl Drop for WebSocketConnectionInitialization {
    fn drop(&mut self) {
        if self.connection.is_none() {
            return;
        }
        let cleanup = if let Some(worker) = self.worker.take() {
            self.finish_worker(WebSocketConnectionClose::INTERNAL, worker)
        } else {
            self.finish_inline(WebSocketConnectionClose::INTERNAL)
        };
        drop(cleanup);
    }
}

fn charge_payload(payload: Option<&Object>) -> Option<usize> {
    let mut charge = SubscriptionMetadataCharge::new();
    if let Some(payload) = payload {
        charge.object(payload)?;
    }
    Some(charge.bytes())
}
