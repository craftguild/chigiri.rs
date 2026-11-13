//! Request middleware and exactly-once source initialization.

use std::sync::Mutex;

use chigiri_core::executor::{
    ArgumentValues, BoxFuture, FieldContext, InternalErrorHandler, RequestContext, RequestNext,
    SubscriptionExecutionInput, SubscriptionSetupDispatch, SubscriptionSetupOutcome,
    establish_subscription_source,
};
use chigiri_core::{
    DeadlineFactory, FieldResult, GraphQlResponse, OperationIdentity, RequestTelemetry,
};

use super::prepared::{PreparedSubscription, SubscriptionStart};
use super::session::{SessionState, SubscriptionSession};
use super::{ErasedSubscription, SubscriptionResolverDispatch};
use crate::ExecutableSchema;
use crate::instrumentation::{InstrumentationKind, instrument_future};

impl<C: Send + Sync + 'static> PreparedSubscription<C> {
    /// Returns the operation identity fixed before source setup.
    pub fn identity(&self) -> &OperationIdentity {
        &self.identity
    }

    /// Runs request and source-setup policies once, borrowing setup state only until return.
    pub fn start<'a>(
        self,
        context: &'a C,
        deadlines: Option<&'a dyn DeadlineFactory>,
    ) -> BoxFuture<'a, SubscriptionStart<C>> {
        let activation = self
            .setup_telemetry
            .as_deref()
            .and_then(RequestTelemetry::instrumentation_scope)
            .cloned();
        instrument_future(activation, async move {
            let PreparedSubscription {
                schema,
                plan,
                request,
                setup_telemetry,
                identity,
                observation_prepared: _,
                observation,
            } = self;
            let telemetry = setup_telemetry
                .as_deref()
                .or_else(|| request.request_telemetry());
            let failure = |message| internal_response(&schema, &identity, telemetry, message);
            if let Some(receipt) = &observation
                && !receipt.wait_for_acceptance().await
            {
                return SubscriptionStart::Response(failure(
                    "subscription observation transfer was not accepted",
                ));
            }
            let Some(dispatch) = schema.inner.subscription.as_deref() else {
                return SubscriptionStart::Response(failure("subscription dispatch is absent"));
            };
            let setup = Setup(dispatch);
            let slot = Mutex::new(None);
            let input = SubscriptionExecutionInput::new(
                schema.inner.resolvers.as_ref(),
                &schema.inner.plugins,
                context,
                Some(&schema.inner.diagnostics),
                schema.inner.diagnostics.lifecycle(),
                telemetry,
                deadlines,
                None,
            );
            let phase = schema.inner.diagnostics.start_phase(
                chigiri_core::LifecyclePhase::RequestPlugin,
                "subscription",
                telemetry,
            );
            let slot_for_terminal = &slot;
            let setup_for_terminal = &setup;
            let failure_for_terminal = &failure;
            let response = RequestNext::new(&schema.inner.plugins, move |_| {
                Box::pin(async move {
                    let outcome =
                        establish_subscription_source(plan, input, setup_for_terminal).await;
                    let response = match &outcome {
                        SubscriptionSetupOutcome::Ready { .. } => empty_response(),
                        SubscriptionSetupOutcome::Response(response) => response.clone(),
                    };
                    match slot_for_terminal.lock() {
                        Ok(mut slot) => *slot = Some(outcome),
                        Err(_) => {
                            return failure_for_terminal(
                                "subscription setup ownership slot is poisoned",
                            );
                        }
                    }
                    response
                })
            })
            .run(RequestContext::new(context, &request))
            .await;
            phase.finish(
                if response.data.is_none() && !response.errors.is_empty() {
                    chigiri_core::LifecycleResult::Rejected
                } else {
                    chigiri_core::LifecycleResult::Succeeded
                },
                Some(&identity),
                None,
            );
            let outcome = slot.into_inner().ok().flatten();
            if response.data.is_some()
                || !response.errors.is_empty()
                || !response.extensions.is_empty()
            {
                return SubscriptionStart::Response(response);
            }
            let Some(SubscriptionSetupOutcome::Ready { root, session }) = outcome else {
                return SubscriptionStart::Response(failure(
                    "subscription setup did not establish a source",
                ));
            };
            let receipt_matches = match (&observation, session.host_observation_receipt()) {
                (None, None) => true,
                (Some(expected), Some(actual)) => expected.same(actual) && !actual.is_closed(),
                _ => false,
            };
            if !receipt_matches {
                return SubscriptionStart::Response(failure(
                    "subscription source observation receipt does not match",
                ));
            }
            let (source, arguments, overflow) = session.into_parts();
            let source_scope = request
                .request_telemetry()
                .and_then(RequestTelemetry::propagation_context)
                .and_then(|values| schema.instrumentation_with_values(values))
                .map(|captured| captured.handoff_scope(InstrumentationKind::SubscriptionSetup));
            let arguments = arguments.with_scope(source_scope.clone());
            let source = super::instrumentation::instrument_source(source, source_scope.clone());
            SubscriptionStart::Ready(Box::new(SubscriptionSession {
                schema,
                root: *root,
                source: Some(source),
                arguments,
                current_event: None,
                overflow,
                identity,
                state: SessionState::Polling,
                observation,
                retired_source: None,
                defer_source_cleanup: false,
                source_instrumentation: source_scope,
            }))
        })
    }
}

struct Setup<'a, C>(&'a dyn SubscriptionResolverDispatch<C>);

impl<C: 'static> SubscriptionSetupDispatch<C, ErasedSubscription> for Setup<'_, C> {
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.0.subscribe(context, arguments)
    }
}

fn internal_response<C>(
    schema: &ExecutableSchema<C>,
    identity: &OperationIdentity,
    telemetry: Option<&RequestTelemetry>,
    message: &'static str,
) -> GraphQlResponse {
    let mut error = crate::private::invariant_error(message);
    schema.inner.diagnostics.handle_internal_error(
        &mut error,
        identity,
        telemetry,
        &[],
        None,
        "subscription-setup",
    );
    GraphQlResponse::request_error(error.to_graphql_error(Vec::new(), Vec::new()))
}

fn empty_response() -> GraphQlResponse {
    GraphQlResponse {
        data: None,
        errors: Vec::new(),
        extensions: crate::Object::new(),
    }
}
