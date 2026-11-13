//! One bounded operation slot driven inline by the socket session.

mod active;
mod cancel;
mod delivery;
mod event;
mod failure;
mod graceful;
mod poll;
mod state;
mod terminal;

#[cfg(test)]
mod tests;

pub(super) use delivery::WebSocketOperationDeliverySeed;
pub(super) use failure::WebSocketOperationSharedFailure;
pub(super) use state::{WebSocketOperationSlot, WebSocketOperationTransition};
