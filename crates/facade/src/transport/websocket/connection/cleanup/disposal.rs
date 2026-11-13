//! Closed connection metadata, without application source or context owners.

use std::sync::Arc;

use chigiri_http::BoxFuture;
use chigiri_runtime::Object;

use super::super::super::request::AdmittedWebSocketRequest;
use super::super::frame::AckGate;
use crate::subscription::control::WebSocketControlStaging;
use crate::subscription::delivery::EndpointByteOwner;

pub(in crate::transport::websocket::connection) struct ConnectionDisposal {
    pub(in crate::transport::websocket::connection) _request: Arc<AdmittedWebSocketRequest>,
    pub(in crate::transport::websocket::connection) _owner: EndpointByteOwner,
    pub(in crate::transport::websocket::connection) _payload: Option<Arc<Object>>,
    pub(in crate::transport::websocket::connection) _controls: Option<WebSocketControlStaging>,
    pub(in crate::transport::websocket::connection) _timers: [Option<BoxFuture<'static, ()>>; 2],
    pub(in crate::transport::websocket::connection) _acknowledgement: Option<Arc<AckGate>>,
}

impl ConnectionDisposal {
    pub(in crate::transport::websocket::connection) fn metadata(
        request: Arc<AdmittedWebSocketRequest>,
        owner: EndpointByteOwner,
    ) -> Self {
        Self {
            _request: request,
            _owner: owner,
            _payload: None,
            _controls: None,
            _timers: [None, None],
            _acknowledgement: None,
        }
    }
}
