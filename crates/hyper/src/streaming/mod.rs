//! Native stream responses and isolated transport failure fixtures.

mod body;
pub(crate) mod response;
#[cfg(test)]
mod service;
pub(crate) mod websocket;

pub use body::HyperStreamingBody;
#[cfg(test)]
pub(crate) use service::streaming_service;
