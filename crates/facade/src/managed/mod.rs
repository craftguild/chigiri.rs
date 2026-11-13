//! Closed SDK lifecycle owners used by the official native services.

mod control;
mod error;
mod http;
mod requests;
mod service;
mod sockets;
mod supervisor;

pub(crate) use control::Control;
pub(crate) use control::StopPhase;
pub use error::ManagedServiceError;
pub(crate) use http::HttpFaultReporter;
pub use http::{ManagedHttpRequest, ManagedHttpResponse};
pub(crate) use requests::RetainedHttpRequest;
pub(crate) use service::Binding;
pub use service::ManagedService;
pub use sockets::ManagedWebSocketUpgrade;
pub(crate) use sockets::{SocketRegistry, SocketScope};
pub(crate) use supervisor::Supervisor;

mod event;
pub(crate) use event::EventActivity;
