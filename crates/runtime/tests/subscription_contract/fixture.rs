use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use chigiri_runtime::__private::{SubscriptionSession, SubscriptionStart};
use chigiri_runtime::{ExecutableSchema, GraphQlRequest, Object};

#[path = "fixture/descriptor.rs"]
mod descriptor;
#[path = "fixture/dispatch.rs"]
mod dispatch;
#[path = "fixture/state.rs"]
mod state;

pub use state::{AppContext, EventMode, SetupMode, SourceStep, State, count};

use descriptor::SCHEMA;
use dispatch::Dispatch;

pub fn schema(state: Arc<State>) -> ExecutableSchema<AppContext> {
    ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(state))
        .build()
        .unwrap()
}

pub fn builder(state: Arc<State>) -> chigiri_runtime::ExecutableSchemaBuilder<AppContext> {
    ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(state))
}

pub fn request() -> GraphQlRequest {
    let mut variables = Object::new();
    variables.insert("limit", 2_i32);
    GraphQlRequest::new(
        "subscription Named($limit: Int!) { chosen: events(limit: $limit) { value } }",
    )
    .with_operation_name("Named")
    .with_variables(variables)
}

pub fn request_fields(fields: &str) -> GraphQlRequest {
    GraphQlRequest::new(format!(
        "subscription Named {{ chosen: events(limit: 2) {{ {fields} }} }}"
    ))
    .with_operation_name("Named")
}

pub fn start(
    schema: &ExecutableSchema<AppContext>,
    context: &AppContext,
) -> SubscriptionSession<AppContext> {
    let prepared = schema.prepare_subscription(request()).unwrap();
    match block_on(prepared.start(context, None)) {
        SubscriptionStart::Ready(session) => *session,
        SubscriptionStart::Response(response) => panic!("setup failed: {response:?}"),
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        match poll_once(future.as_mut()) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

pub fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    future.poll(&mut context)
}
