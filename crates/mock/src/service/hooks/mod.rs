//! Run the fixed request, operation, and field hook phases around the endpoint pipeline.

mod completion;
mod copy_budget;
mod copying;
mod diagnostics;
mod field;
mod http;
mod observation;
mod operation;
mod progress;
mod request_view;
mod session;
mod subscription_observation;
mod terminal;
pub use terminal::TerminalDiagnostic;
#[cfg(test)]
mod tests;
mod views;

pub(super) use completion::{prepared, prepared_scoped};
#[cfg(test)]
pub(in crate::service) use field::exhaust_projection_for_test;
pub(super) use field::{establish_subscription, project_event, resolve};
pub use http::HttpScope;
pub(crate) use http::HttpScopes;
pub(super) use http::Observe;
pub(in crate::service) use session::Session;
pub(in crate::service) use subscription_observation::{
    EstablishedParts, EstablishedSubscription, RegistrationFence,
    prepare as prepare_subscription_observation,
};

use std::sync::Arc;

use chigiri::core::executor::ResolvedValue;
use chigiri::{
    BoxFuture, FieldContext, FieldNext, FieldResult, GraphQlResponse, OperationContext,
    OperationNext, PathSegment, Plugin, RequestContext, RequestNext,
};

use super::Client;
use crate::hooks::HookSlot;
use crate::luau::ScriptError;
use crate::runtime::HookLog;

/// Sanitized hook location plus either a log record or fixed diagnostic.
pub struct HookEvent {
    /// Mock-specific request correlation ID.
    pub request_id: u64,
    /// One of the six executed hook slots.
    pub slot: HookSlot,
    /// Relative module name from the verified layout; no physical path is retained.
    pub module: Option<String>,
    /// GraphQL field coordinate, when applicable.
    pub coordinate: Option<Arc<str>>,
    /// Field response path; request bodies, arguments, and parent values are excluded.
    pub path: Arc<[PathSegment]>,
    /// User-supplied structured log record or a sanitized script error.
    pub record: Result<HookLog, ScriptError>,
}
/// Sanitized resolver failure location with no input values or raw exception text.
#[derive(serde::Serialize)]
pub struct ResolverDiagnostic {
    /// Stack captured at the failure boundary, when available.
    pub stack: Option<crate::luau::ScriptStack>,
    /// Mock-specific request correlation ID.
    pub request_id: u64,
    /// Resolver's schema coordinate.
    pub coordinate: String,
    /// Sanitized relative module name, without a physical path.
    pub module: Option<String>,
    /// Diagnostic phase such as type checking or runtime execution.
    pub phase: &'static str,
    /// Stable failure classification independent of VM wording.
    pub cause: &'static str,
    /// Schema-owned member associated with the failure, when known.
    pub member: Option<String>,
    /// Schema-derived expected shape, when known.
    pub expected: Option<String>,
    /// Safe structural category observed at the boundary, when known.
    pub actual: Option<String>,
    /// Fixed host-owned correction guidance, when available.
    pub hint: Option<String>,
    /// The verified line in the Luau module, or null for native/unknown sites.
    pub line: Option<usize>,
    /// The verified column in the Luau module, or null for native/unknown sites.
    pub column: Option<usize>,
    /// Whether cancellation interrupted post-processing.
    pub cancelled: bool,
}
type DiagnosticSink = Arc<dyn Fn(ResolverDiagnostic) + Send + Sync>;
type EventSink = Arc<dyn Fn(HookEvent) + Send + Sync>;

pub(super) struct Hooks;

impl Plugin<Client> for Hooks {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, Client>,
        next: RequestNext<'a, Client>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let client = context.context();
            let session = client
                .session
                .clone()
                .unwrap_or_else(|| session::Session::new(client));
            let current = session.clone();
            session::REQUEST
                .scope(session, async move {
                    current.finish(next.run(context).await).await
                })
                .await
        })
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, Client>,
        next: OperationNext<'a, Client>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            if session::REQUEST.try_with(|_| ()).is_ok() {
                return operation::run(context, next).await;
            }
            // Event execution starts at operation middleware, not raw-request
            // middleware. Give its six Mock hook phases the fresh factory owner.
            let client = context.context();
            let session = client
                .session
                .clone()
                .unwrap_or_else(|| Session::new(client));
            let current = session.clone();
            session::REQUEST
                .scope(session, async move {
                    current.finish(operation::run(context, next).await).await
                })
                .await
        })
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, Client>,
        next: FieldNext<'a, Client>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(field::run(context, next))
    }
}

fn error(message: &'static str) -> ScriptError {
    ScriptError::new("hook", message)
}
