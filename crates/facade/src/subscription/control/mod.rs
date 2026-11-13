//! Bounded control-frame staging shared by official streaming bridges.

#[cfg(any(feature = "subscription-worker", test))]
mod completion_fallback;
mod encoding;
mod frame;
mod heartbeat;
mod http_terminal;
mod signal;
mod websocket;

pub use frame::SubscriptionControlFrame;
pub use heartbeat::HttpHeartbeatStaging;
pub use http_terminal::{HttpSubscriptionTerminal, HttpTerminalStaging};
pub use signal::SubscriptionControlSignal;
pub use websocket::{
    WebSocketControlClass, WebSocketControlStaging, WebSocketWriterSchedule, WebSocketWriterTurn,
};

#[cfg(feature = "subscription-worker")]
pub(crate) use completion_fallback::{
    InitialCompletionFallbackCharge, InitialCompletionFallbacks, InitialPreparationByteGuards,
    InitialResponseBufferGuard, MAX_INITIAL_RESPONSE_BYTES, initial_response_charge,
    reserve_initial_preparation,
};

#[cfg(test)]
mod tests;
