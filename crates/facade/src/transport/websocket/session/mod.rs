//! One application-polled socket owner shared by the official bridges.

mod active;
mod closing;
mod driver;
mod events;
mod opening;
mod operation;
mod release;
mod socket;
mod writer;

#[cfg(test)]
mod tests;

pub use driver::WebSocketSessionDriver;
pub use opening::{WebSocketOpeningDriver, WebSocketOpeningOutcome};
pub use socket::{
    WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketWireFrame, WebSocketWriteProgress,
};
