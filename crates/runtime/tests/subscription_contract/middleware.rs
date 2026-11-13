use std::sync::Arc;
use std::task::Poll;

use chigiri_runtime::__private::{BoxFuture, ResolvedValue, SubscriptionStart};
use chigiri_runtime::{
    FieldContext, FieldError, FieldNext, FieldResult, GraphQlError, GraphQlResponse,
    OperationContext, OperationNext, Plugin, RequestContext, RequestNext,
};

use super::fixture::{AppContext, SourceStep, State, block_on, builder, count, poll_once, request};

struct RejectAfterSetup;

impl Plugin<AppContext> for RejectAfterSetup {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, AppContext>,
        next: RequestNext<'a, AppContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let inner = next.run(context).await;
            assert!(inner.data.is_none());
            assert!(inner.errors.is_empty());
            GraphQlResponse::request_error(
                GraphQlError::new("request policy rejected after setup").code("POLICY_REJECTED"),
            )
        })
    }
}

#[test]
fn request_plugin_rejection_after_capture_drops_the_established_source() {
    assert_post_capture_rejection(RejectAfterSetup, "request policy rejected after setup");
}

struct RejectOperationAfterSetup;

impl Plugin<AppContext> for RejectOperationAfterSetup {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, AppContext>,
        next: OperationNext<'a, AppContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let inner = next.run(context).await;
            assert!(inner.data.is_none());
            assert!(inner.errors.is_empty());
            GraphQlResponse::request_error(
                GraphQlError::new("operation policy rejected after setup").code("POLICY_REJECTED"),
            )
        })
    }
}

struct RejectRootAfterSetup;

impl Plugin<AppContext> for RejectRootAfterSetup {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        next: FieldNext<'a, AppContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            assert_eq!(context.parent_type(), "Subscription");
            next.run(context).await?;
            Err(FieldError::public("root policy rejected after setup").code("POLICY_REJECTED"))
        })
    }
}

#[test]
fn operation_and_root_postprocessing_drop_captured_sources_on_rejection() {
    assert_post_capture_rejection(
        RejectOperationAfterSetup,
        "operation policy rejected after setup",
    );
    assert_post_capture_rejection(RejectRootAfterSetup, "root policy rejected after setup");
}

struct PendingRootAfterSetup;

impl Plugin<AppContext> for PendingRootAfterSetup {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        next: FieldNext<'a, AppContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            next.run(context).await?;
            std::future::pending().await
        })
    }
}

#[test]
fn cancelling_start_after_root_capture_drops_the_source() {
    let state = State::new(vec![SourceStep::Event(1)]);
    let schema = builder(Arc::clone(&state))
        .plugin(PendingRootAfterSetup)
        .build()
        .unwrap();
    let context = AppContext::new(0);
    let prepared = schema.prepare_subscription(request()).unwrap();
    let mut future = prepared.start(&context, None);

    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    assert_eq!(count(&state.counts.setup_calls), 1);
    assert_eq!(count(&state.counts.source_drops), 0);
    drop(future);
    assert_eq!(count(&state.counts.source_drops), 1);
}

fn assert_post_capture_rejection(plugin: impl Plugin<AppContext>, message: &str) {
    let state = State::new(vec![SourceStep::Event(1)]);
    let schema = builder(Arc::clone(&state)).plugin(plugin).build().unwrap();
    let context = AppContext::new(0);
    let prepared = schema.prepare_subscription(request()).unwrap();
    let response = match block_on(prepared.start(&context, None)) {
        SubscriptionStart::Response(response) => response,
        SubscriptionStart::Ready(_) => panic!("request rejection leaked a ready session"),
    };

    assert_eq!(count(&state.counts.setup_calls), 1);
    assert_eq!(count(&state.counts.source_polls), 0);
    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(response.errors[0].message, message);
}
