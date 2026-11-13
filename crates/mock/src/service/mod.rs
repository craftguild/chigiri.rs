//! Connect IR-based concrete-field dispatch to a bounded worker and owned endpoint.

#[cfg(test)]
mod blog_fixture;
mod control;
mod dispatch;
mod hooks;
mod native;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_authoring_boundaries;
#[cfg(test)]
mod tests_reference_guidance;
mod worker;

pub use control::ServerControl;
pub(crate) use dispatch::managed_endpoint;
pub use dispatch::{endpoint, endpoint_with_scopes};
pub(crate) use hooks::HttpScopes;
pub use hooks::{HookEvent, HttpScope, ResolverDiagnostic, TerminalDiagnostic};
pub use worker::{Client, Resolution, Worker};
