//! Authentication, query-aware rate limiting, context, and cookie hooks.

use std::sync::Arc;

use chigiri::GraphQlError;
use chigiri_http::{
    AdmissionDecision, AdmissionError, BoxFuture, ContextFactory, HookError, PrepareError,
    PreparedContext, Rejection, RequestContext, ResponseHeaders, ResponseHook, ResponseHookContext,
    TransportAdmission, TransportRequest,
};

use crate::application::{
    AppContext, CookieCommands, ItemLoader, ResponseState, authenticated_viewer,
};

pub struct RequestPolicy;

impl TransportAdmission for RequestPolicy {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(async move {
            if let Some(value) = request
                .headers()
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case("authorization"))
                && value.value != b"Bearer synthetic-token"
            {
                return Ok(AdmissionDecision::Reject(rejection(
                    401,
                    "Authentication failed",
                    "UNAUTHENTICATED",
                )));
            }
            Ok(AdmissionDecision::Continue)
        })
    }
}

fn rejection(status: u16, message: &'static str, code: &'static str) -> Rejection {
    Rejection::new(status, GraphQlError::new(message).code(code))
        .expect("The fixed status of 400 to 599 is for rejection.")
}

pub struct ApplicationContextFactory;

impl ContextFactory<AppContext, ResponseState> for ApplicationContextFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, ResponseState>, PrepareError>> {
        let authenticated = request
            .transport()
            .headers()
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("authorization"));
        let has_query_budget = request.transport().headers().iter().any(|header| {
            header.name.eq_ignore_ascii_case("x-query-budget") && header.value == b"allow"
        });
        Box::pin(async move {
            if request.request().query.contains("expensive") && !has_query_budget {
                return Err(PrepareError::Reject(rejection(
                    429,
                    "Query budget exceeded",
                    "RATE_LIMITED",
                )));
            }
            let cookies = Arc::new(CookieCommands::default());
            Ok(PreparedContext::new(
                AppContext {
                    viewer: authenticated.then(authenticated_viewer),
                    loader: Arc::new(ItemLoader::synthetic()),
                    cookies: Arc::clone(&cookies),
                },
                cookies,
            ))
        })
    }
}

pub struct CookieResponseHook;

impl ResponseHook<ResponseState> for CookieResponseHook {
    fn call<'a>(
        &'a self,
        context: ResponseHookContext<'a, ResponseState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        let cookies = context.state().take();
        Box::pin(async move {
            let mut headers = ResponseHeaders::new();
            for cookie in cookies {
                headers
                    .append("set-cookie", &cookie)
                    .map_err(HookError::new)?;
            }
            Ok(headers)
        })
    }
}
