//! Integrated production-read-only tests for the sequential subscription owner.

mod deadlines;
mod descriptor;
mod dispatch;
mod event;
mod harness;
#[cfg(feature = "subscription-worker")]
mod http_worker;
#[cfg(feature = "subscription-worker")]
mod observation;
mod owned_input;
mod refresh;
#[cfg(feature = "subscription-worker")]
mod retained_ids;
mod setup;
mod state;
#[cfg(feature = "subscription-worker")]
mod websocket_session;
#[cfg(feature = "subscription-worker")]
mod websocket_worker;
