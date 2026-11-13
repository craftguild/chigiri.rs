//! Application-polled WebSocket initialization and acknowledgement ownership.

mod acknowledgement;
mod authorization;
mod cleanup;
mod close;
mod frame;
mod handoff;
mod initialization;
mod initialized;
mod input;
mod observation;
mod protocol;
mod state;

pub use acknowledgement::AckPendingWebSocketConnection;
pub use cleanup::WebSocketConnectionCleanup;
pub use close::WebSocketConnectionClose;
pub use frame::{
    WebSocketAcknowledgementFrame, WebSocketAcknowledgementProgress, WebSocketConnectionFrame,
};
pub use handoff::{WebSocketConnectionHandoff, WebSocketConnectionHandoffResolution};
pub use initialization::WebSocketConnectionInitialization;
pub use initialized::InitializedWebSocketConnection;
pub use input::{InitializedWebSocketInput, WebSocketInitializationInput};
pub use state::{WebSocketAcknowledgementResolution, WebSocketConnectionResolution};

pub(crate) use authorization::AuthorizationErrorObserver;
pub(crate) use handoff::WebSocketConnectionHandoff as Handoff;
pub(crate) use initialized::InitializedWebSocketConnectionParts;
pub(crate) use observation::native as native_socket_observer;
pub(crate) use observation::new as observer;
