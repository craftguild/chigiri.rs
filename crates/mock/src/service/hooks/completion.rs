//! Bind request failures, encode confirmation, and committed responses to one owner.

use std::sync::{Arc, atomic::Ordering};

use chigiri::{GraphQlError, GraphQlResponse, HttpRequestContext, PreparedContext, Value};
use chigiri_http::{
    ResponseCompletion, ResponseCompletionOutcome, ResponseMediaType, ResponseParts,
};

use super::session::Session;
use crate::service::Client;

#[derive(Clone)]
pub(super) enum Failure {
    Internal,
    SourceStopped,
    Timeout,
    Limit(crate::limits::request::LimitNotice),
    LocalLimit(crate::limits::request::LocalLimit),
    OutputLimit,
    Business(crate::runtime::business::BusinessFailure),
    Rejected(u16, GraphQlError),
}

impl Failure {
    pub(super) fn rejected(rejection: crate::hooks::Rejection) -> Self {
        Self::Rejected(
            rejection.status,
            GraphQlError::new(rejection.message)
                .code("MOCK_REJECTED")
                .extension("httpStatus", Value::Int(rejection.status.into())),
        )
    }

    pub(super) fn include_rejection(
        current: Option<Self>,
        rejection: Option<crate::hooks::Rejection>,
    ) -> Option<Self> {
        let Some(rejection) = rejection else {
            return current;
        };
        match current {
            None => Some(Self::rejected(rejection)),
            Some(Self::Rejected(old, _)) if old < rejection.status => {
                Some(Self::rejected(rejection))
            }
            _ => current,
        }
    }

    fn status(&self) -> u16 {
        match self {
            Self::Business(_) => 200,
            Self::Rejected(status, _) => *status,
            _ => 500,
        }
    }

    fn error(&self) -> GraphQlError {
        match self {
            Self::Internal => {
                GraphQlError::new("Mock execution failed").code("MOCK_EXECUTION_FAILED")
            }
            Self::SourceStopped => {
                GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR")
            }
            Self::Timeout => {
                GraphQlError::new("Mock execution timed out").code("MOCK_EXECUTION_TIMEOUT")
            }
            Self::Limit(notice) => GraphQlError::new("Mock execution limit exceeded")
                .code("MOCK_EXECUTION_LIMIT_EXCEEDED")
                .extension("limit", crate::limits::request::policy(notice.kind).name),
            Self::LocalLimit(limit) => GraphQlError::new("Mock execution limit exceeded")
                .code("MOCK_EXECUTION_LIMIT_EXCEEDED")
                .extension("limit", limit.name()),
            Self::OutputLimit => GraphQlError::new("Mock execution limit exceeded")
                .code("MOCK_EXECUTION_LIMIT_EXCEEDED")
                .extension("limit", "output"),
            Self::Business(failure) => {
                let mut error = GraphQlError::new(&failure.message).code(&failure.code);
                error.path = failure.path.to_vec();
                error
            }
            Self::Rejected(_, error) => error.clone(),
        }
    }
}

pub(in crate::service) fn prepared(
    mut client: Client,
    context: HttpRequestContext<'_>,
) -> PreparedContext<Client, ()> {
    let session = Session::with_http(&client, true);
    if session.request {
        super::http::prepare(&session.trace, context);
    }
    client.session = Some(session.clone());
    PreparedContext::execution_only(client).with_response_completion(Completion(session))
}

pub(in crate::service) fn prepared_scoped(
    mut client: Client,
    context: HttpRequestContext<'_>,
    scope: super::HttpScope,
) -> PreparedContext<Client, ()> {
    let initial_http = matches!(
        context.phase(),
        chigiri::RequestPhase::Finite
            | chigiri::RequestPhase::SubscriptionSetup {
                transport: chigiri_http::SubscriptionTransport::ServerSentEvents
                    | chigiri_http::SubscriptionTransport::Multipart,
            }
    );
    let session = Session::with_scope(&client, initial_http, scope);
    if session.request {
        super::http::prepare(&session.trace, context);
    }
    client.session = Some(session.clone());
    let prepared = PreparedContext::execution_only(client);
    if initial_http {
        prepared.with_response_completion(Completion(session))
    } else {
        prepared
    }
}

struct Completion(Arc<Session>);

impl ResponseCompletion for Completion {
    fn complete(
        &self,
        mut response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        let session = &self.0;
        if !session.started.load(Ordering::Acquire) {
            session.report_terminal(Some(response.status), "response");
            return response;
        }
        session.observe_execution_failure();
        match outcome {
            ResponseCompletionOutcome::TimedOut => session.fail(Failure::Timeout),
            ResponseCompletionOutcome::Failed => session.fail(Failure::Internal),
            ResponseCompletionOutcome::EncodingLimitExceeded => session.fail(Failure::OutputLimit),
            ResponseCompletionOutcome::Encoded => {}
        }
        if session.failure.lock().expect("Request failed").is_none() {
            session.commit();
        }
        if let Some(failure) = session.failure.lock().expect("Request failed").clone() {
            let graph = session.failure_response(&failure);
            let encoded =
                chigiri_http::encode_response(ResponseMediaType::GraphQlResponseJson, &graph)
                    .unwrap_or_else(|_| {
                        ResponseParts::serialization_failure(ResponseMediaType::GraphQlResponseJson)
                    });
            response.status = failure.status();
            response.body = encoded.body;
            response
                .headers
                .retain(|header| header.name != "content-length");
            if !matches!(failure, Failure::Rejected(..)) {
                session.trace.transport_failed();
            }
        }
        session.report_terminal(Some(response.status), "response");
        response
    }
}

impl Session {
    pub(in crate::service) fn event_execution_failed(&self) -> bool {
        let Ok(failure) = self.failure.lock() else {
            return true;
        };
        matches!(
            *failure,
            Some(
                Failure::Internal
                    | Failure::SourceStopped
                    | Failure::Timeout
                    | Failure::Limit(_)
                    | Failure::LocalLimit(_)
                    | Failure::OutputLimit
            )
        )
    }

    /// Recover failures retained in the request ledger when a sibling future drops before delivery.
    pub(super) fn observe_execution_failure(&self) {
        use crate::limits::request::BudgetFailure;
        if self.source_stopped() {
            self.fail(Failure::SourceStopped);
            return;
        }
        if let Some(rejection) = self
            .client
            .get()
            .and_then(Client::rejections)
            .and_then(|rejections| rejections.get())
        {
            self.trace.reject(rejection.status);
            self.fail(Failure::rejected(rejection));
        }
        if let Some(failure) = self.trace.execution_budget.business() {
            self.fail(Failure::Business(failure));
        }
        if matches!(
            self.trace.execution_budget.failure(),
            Some(
                BudgetFailure::Execution
                    | BudgetFailure::Hard(_)
                    | BudgetFailure::LocalHard(_)
                    | BudgetFailure::Deadline
            )
        ) {
            self.fail(Failure::Internal);
        }
    }

    pub(super) fn fail(&self, mut failure: Failure) {
        if self.source_stopped() {
            failure = Failure::SourceStopped;
        } else if matches!(failure, Failure::Internal) {
            use crate::limits::request::BudgetFailure;
            failure = match self.trace.execution_budget.failure() {
                Some(BudgetFailure::Hard(notice)) => Failure::Limit(notice),
                Some(BudgetFailure::LocalHard(limit)) => Failure::LocalLimit(limit),
                Some(BudgetFailure::Deadline) => Failure::Timeout,
                _ if std::time::Instant::now() >= self.trace.deadline => Failure::Timeout,
                _ => failure,
            };
        }
        {
            let mut current = self.failure.lock().expect("Request failed");
            let replace = match (&*current, &failure) {
                (None, _) => true,
                (_, Failure::SourceStopped) => true,
                (Some(Failure::Rejected(old, _)), Failure::Rejected(new, _)) => new > old,
                (Some(Failure::Business(_)), Failure::Rejected(new, _)) => *new > 200,
                (Some(Failure::Rejected(old, _)), Failure::Business(_)) => *old < 200,
                (Some(Failure::Business(_)), Failure::Business(_)) => false,
                (Some(Failure::Rejected(..) | Failure::Business(_)), _) => true,
                _ => false,
            };
            if replace {
                *current = Some(failure);
            }
        }
        self.trace.cancel.cancel();
        if let Some(client) = self.client.get() {
            client.abort_request();
        }
    }

    pub(super) fn commit(&self) {
        if self.client.get().is_none_or(|client| {
            match &self.trace.subscription_logs {
                Some(logger) => client.commit_request_logged(self.trace.id, logger),
                None => client.commit_request(),
            }
            .is_err()
        }) {
            self.fail(Failure::Internal);
        }
    }

    pub(super) fn failure_response(&self, failure: &Failure) -> GraphQlResponse {
        GraphQlResponse::executed(Value::Null).with_error(
            failure
                .error()
                .extension("requestId", self.trace.id.to_string()),
        )
    }

    pub(super) async fn start(&self, client: &Client, mutation: bool) -> bool {
        self.started.store(true, Ordering::Release);
        let execution = match self.execution(client) {
            Ok(execution) => execution,
            Err(_) => {
                self.fail(Failure::Internal);
                return false;
            }
        };
        match client.request_client(mutation, &execution).await {
            Ok(client) => self.client.set(client).is_ok(),
            Err(_) => {
                self.fail(Failure::Internal);
                false
            }
        }
    }

    pub(super) async fn finish(&self, mut response: GraphQlResponse) -> GraphQlResponse {
        if !self.started.load(Ordering::Acquire) {
            return response;
        }
        self.observe_execution_failure();
        if !response.errors.is_empty() && self.failure.lock().expect("Request failed").is_none() {
            self.fail(Failure::Internal);
        }
        if self.failure.lock().expect("Request failed").is_none() {
            match self.client.get() {
                Some(client) if client.prepare_request(self.trace.deadline).await.is_ok() => {}
                _ => self.fail(Failure::Internal),
            }
            if !self.http && self.failure.lock().expect("Request failed").is_none() {
                self.commit();
            }
        }
        if self.failure.lock().expect("Request failed").is_some() {
            if let Some(client) = self.client.get()
                && !client.settle_request(self.trace.deadline).await
            {
                self.fail(Failure::Timeout);
            }
            // Preserve sibling-future failures discovered during teardown instead of hiding them behind rejection.
            self.observe_execution_failure();
        }
        if let Some(failure) = self.failure.lock().expect("Request failed").clone() {
            response = self.failure_response(&failure);
        }
        if !self.http {
            self.report_terminal(None, "response");
        }
        response
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.trace.cancel.cancel();
        if let Some(client) = self.client.get() {
            client.abort_request();
        }
        self.report_terminal(None, "drop");
    }
}
