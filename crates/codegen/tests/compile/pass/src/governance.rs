//! Verify trait execution retains normal plugins, completion, and resource limits.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use chigiri::{FieldContext, FieldNext, FieldResult, Plugin};

use super::{MutationFields, QueryFields, TeamFields, Types, UserFields, generated};

#[test]
fn trait_completion_and_policy_not_circumvented() {
    let fields = Arc::new(AtomicUsize::new(0));
    let mut limits = chigiri::ExecutionLimits::production();
    limits.max_output_nodes = 2;
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .execution_limits(limits)
        .push_plugin(CountFields(Arc::clone(&fields)))
        .without_context()
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(
        chigiri::GraphQlRequest::new(r#"{ user(id: "user-1") { id name updatedAt } }"#),
        (),
        &NeverDeadline,
    ));

    assert!(fields.load(Ordering::SeqCst) > 0);
    assert!(response.errors.iter().any(|error| {
        error.extensions.get("code") == Some(&chigiri::Value::from("INTERNAL_SERVER_ERROR"))
    }));
}

fn resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_user(UserFields)
        .with_team(TeamFields)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .build()
}

struct CountFields(Arc<AtomicUsize>);

impl Plugin<()> for CountFields {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, FieldResult<chigiri::__private::ResolvedValue>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

struct NeverDeadline;

impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
