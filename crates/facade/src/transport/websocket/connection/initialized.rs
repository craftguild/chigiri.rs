//! Closed post-ack input classifier and physical control ownership.

use std::sync::Arc;

use chigiri_http::DeadlineFactory;
use chigiri_runtime::{Object, RequestTelemetry};

use super::super::request::AdmittedWebSocketRequest;
use super::authorization::AuthorizationErrorObserver;
use super::cleanup::{self, WebSocketConnectionCleanup};
use super::close::WebSocketConnectionClose;
use super::frame::WebSocketConnectionFrame;
use super::input::InitializedWebSocketInput;
use super::protocol;
use super::state::begin_control;
use crate::subscription::bridge_resources::{
    BridgeConnection, BridgeConnectionAdmission, BridgeResourceError,
};
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::delivery::EndpointByteOwner;

/// A physically acknowledged connection that may admit GraphQL operations.
#[must_use = "an initialized connection must be driven or explicitly cleaned up"]
#[doc(hidden)]
pub struct InitializedWebSocketConnection {
    connection: Option<BridgeConnection>,
    request: Arc<AdmittedWebSocketRequest>,
    owner: EndpointByteOwner,
    payload: Option<Arc<Object>>,
    payload_charge: usize,
    controls: Option<Box<WebSocketControlStaging>>,
    deadlines: &'static dyn DeadlineFactory,
    observer: Arc<dyn AuthorizationErrorObserver>,
    terminal: Option<WebSocketConnectionClose>,
}

impl InitializedWebSocketConnection {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
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
            controls: Some(Box::new(controls)),
            deadlines,
            observer,
            terminal: None,
        }
    }

    /// Classifies one bounded client message through the authoritative decoder.
    pub fn receive_text(
        &mut self,
        text: &str,
    ) -> Result<InitializedWebSocketInput, WebSocketConnectionClose> {
        protocol::receive_text(
            self.controls
                .as_deref_mut()
                .expect("live initialized controls"),
            &self.observer,
            &mut self.terminal,
            text,
        )
    }

    /// Rejects one binary GraphQL protocol message.
    pub fn receive_binary(&mut self) -> WebSocketConnectionClose {
        protocol::receive_binary(&mut self.terminal)
    }

    /// Begins one immutable control frame while retaining physical connection quota.
    pub fn begin_control_send(
        &mut self,
    ) -> Result<Option<WebSocketConnectionFrame>, WebSocketConnectionClose> {
        begin_control(
            self.connection
                .as_ref()
                .expect("live initialized connection"),
            self.controls
                .as_deref_mut()
                .expect("live initialized controls"),
        )
        .map_err(|()| protocol::fail_framework(&self.observer, &mut self.terminal))
    }

    /// Returns a weak operation admission view for the future session driver.
    pub fn operation_admission(&self) -> BridgeConnectionAdmission {
        self.connection
            .as_ref()
            .expect("live initialized connection")
            .admission()
    }

    pub(crate) fn into_session_parts(
        mut self,
        expected: &EndpointByteOwner,
    ) -> Result<InitializedWebSocketConnectionParts, BridgeResourceError> {
        if !self.owner.same_owner(expected) {
            return Err(BridgeResourceError::StaleGeneration);
        }
        Ok(InitializedWebSocketConnectionParts {
            connection: self
                .connection
                .take()
                .ok_or(BridgeResourceError::Invariant)?,
            request: Arc::clone(&self.request),
            owner: self.owner.clone(),
            initialization_payload: self.payload.take(),
            controls: *self.controls.take().ok_or(BridgeResourceError::Invariant)?,
            deadlines: self.deadlines,
            observer: Arc::clone(&self.observer),
            terminal: self.terminal,
        })
    }

    #[cfg(test)]
    pub(crate) fn into_session_parts_for_test(
        self,
    ) -> Result<InitializedWebSocketConnectionParts, BridgeResourceError> {
        let expected = self.owner.clone();
        self.into_session_parts(&expected)
    }

    /// Returns the retained request telemetry without exposing credentials.
    pub fn request_telemetry(&self) -> &RequestTelemetry {
        self.request.telemetry()
    }

    /// Returns the optional accepted initialization object.
    pub fn initialization_payload(&self) -> Option<&Object> {
        self.payload.as_deref()
    }

    /// Returns the checked logical metadata charge of the initialization object.
    pub const fn initialization_payload_charge(&self) -> usize {
        self.payload_charge
    }

    /// Revokes logical admission and returns application-polled cleanup.
    pub fn into_cleanup(mut self) -> WebSocketConnectionCleanup {
        let close = self.terminal.unwrap_or(WebSocketConnectionClose::INTERNAL);
        self.cleanup(close)
    }

    fn cleanup(&mut self, close: WebSocketConnectionClose) -> WebSocketConnectionCleanup {
        cleanup::framework(
            self.connection.take().expect("live initialized connection"),
            cleanup::disposal::ConnectionDisposal {
                _payload: self.payload.take(),
                _controls: self.controls.take().map(|controls| *controls),
                ..cleanup::disposal::ConnectionDisposal::metadata(
                    Arc::clone(&self.request),
                    self.owner.clone(),
                )
            },
            close,
            self.deadlines,
        )
    }
}

pub(crate) struct InitializedWebSocketConnectionParts {
    pub(crate) connection: BridgeConnection,
    pub(crate) request: Arc<AdmittedWebSocketRequest>,
    pub(crate) owner: EndpointByteOwner,
    pub(crate) initialization_payload: Option<Arc<Object>>,
    pub(crate) controls: WebSocketControlStaging,
    pub(crate) deadlines: &'static dyn DeadlineFactory,
    pub(crate) observer: Arc<dyn AuthorizationErrorObserver>,
    terminal: Option<WebSocketConnectionClose>,
}

impl InitializedWebSocketConnectionParts {
    /// Moves the same sole control queue to cancelled-operation supervision.
    pub(crate) fn into_shutdown(mut self) -> (WebSocketConnectionCleanup, WebSocketControlStaging) {
        let close = self.terminal.unwrap_or(WebSocketConnectionClose::INTERNAL);
        self.controls.cancel();
        let cleanup = cleanup::framework(
            self.connection,
            cleanup::disposal::ConnectionDisposal {
                _payload: self.initialization_payload.take(),
                ..cleanup::disposal::ConnectionDisposal::metadata(self.request, self.owner)
            },
            close,
            self.deadlines,
        );
        (cleanup, self.controls)
    }

    pub(crate) fn receive_text(
        &mut self,
        text: &str,
    ) -> Result<InitializedWebSocketInput, WebSocketConnectionClose> {
        protocol::receive_text(&mut self.controls, &self.observer, &mut self.terminal, text)
    }

    pub(crate) fn receive_binary(&mut self) -> WebSocketConnectionClose {
        protocol::receive_binary(&mut self.terminal)
    }

    pub(crate) fn request_close(
        &mut self,
        close: WebSocketConnectionClose,
    ) -> WebSocketConnectionClose {
        protocol::request_close(&mut self.terminal, close)
    }

    pub(crate) fn operation_admission(&self) -> BridgeConnectionAdmission {
        self.connection.admission()
    }

    #[cfg(test)]
    pub(crate) fn into_cleanup(mut self) -> WebSocketConnectionCleanup {
        let close = self.terminal.unwrap_or(WebSocketConnectionClose::INTERNAL);
        cleanup::framework(
            self.connection,
            cleanup::disposal::ConnectionDisposal {
                _payload: self.initialization_payload.take(),
                _controls: Some(self.controls),
                ..cleanup::disposal::ConnectionDisposal::metadata(self.request, self.owner)
            },
            close,
            self.deadlines,
        )
    }
}

impl Drop for InitializedWebSocketConnection {
    fn drop(&mut self) {
        if self.connection.is_some() {
            drop(self.cleanup(WebSocketConnectionClose::INTERNAL));
        }
    }
}
