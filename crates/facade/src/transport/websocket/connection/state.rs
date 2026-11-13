//! Closed transitions between authorization, acknowledgement, and operation input.

use super::acknowledgement::AckPendingWebSocketConnection;
use super::cleanup::WebSocketConnectionCleanup;
use super::frame::WebSocketConnectionFrame;
use super::initialized::InitializedWebSocketConnection;
use crate::subscription::bridge_resources::BridgeConnection;
use crate::subscription::control::WebSocketControlStaging;

/// Resolution of the authorization phase.
#[doc(hidden)]
pub enum WebSocketConnectionResolution {
    /// Authorization succeeded and the sole acknowledgement awaits a writer.
    Acknowledgement(AckPendingWebSocketConnection),
    /// The connection closed and retains its physical cleanup owner.
    Closing(WebSocketConnectionCleanup),
}

/// Resolution after the acknowledgement writer is finished.
#[doc(hidden)]
pub enum WebSocketAcknowledgementResolution {
    /// A successful physical flush now permits operation admission.
    Initialized(InitializedWebSocketConnection),
    /// The acknowledgement failed and the connection is closing.
    Closing(WebSocketConnectionCleanup),
}

pub(super) fn begin_control(
    connection: &BridgeConnection,
    controls: &mut WebSocketControlStaging,
) -> Result<Option<WebSocketConnectionFrame>, ()> {
    let Some(frame) = controls.begin_send().map_err(|_| ())? else {
        return Ok(None);
    };
    let physical = connection.physical_lease().map_err(|_| ())?;
    Ok(Some(WebSocketConnectionFrame::new(frame, physical)))
}
