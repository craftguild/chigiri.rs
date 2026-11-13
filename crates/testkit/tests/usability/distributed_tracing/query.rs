//! Ordinary generated resolver code uses only standard Context APIs.

use std::future::poll_fn;
use std::task::Poll;

use opentelemetry::Context;
use opentelemetry::trace::TraceContextExt;

use super::super::generated;
use super::super::support::{Item, ItemFields, MutationFields, Types};
use super::PrivateValue;
use generated::QueryResolver as _;

struct Query {
    fail: bool,
}

impl generated::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        let current = Context::current();
        let before = current.span().span_context().clone();
        let suppressed = current.is_telemetry_suppressed();
        assert!(current.get::<PrivateValue>().is_none());
        let mut pending = true;
        poll_fn(|task| {
            if pending {
                pending = false;
                task.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        let after = Context::current();
        assert_eq!(*after.span().span_context(), before);
        assert_eq!(after.is_telemetry_suppressed(), suppressed);
        assert!(after.get::<PrivateValue>().is_none());
        if self.fail {
            return Err(chigiri::FieldError::internal(std::io::Error::other(
                "private error source",
            )));
        }
        Ok(format!(
            "{}:{}:{}",
            before.trace_id(),
            before.span_id(),
            suppressed
        ))
    }

    async fn item<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> chigiri::FieldResult<Item> {
        Ok(Item {
            id: "item-1".into(),
            name: "Chigiri".to_owned(),
        })
    }
}

pub(super) fn resolvers() -> generated::Resolvers<(), Types> {
    build(false)
}

pub(super) fn failing_resolvers() -> generated::Resolvers<(), Types> {
    build(true)
}

pub(super) fn one_shot_resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query { fail: false })
        .with_mutation(Mutation)
        .with_item(ItemFields)
        .build()
}

struct Mutation;
impl generated::MutationResolver<(), Types, chigiri::FieldError> for Mutation {
    async fn rename<'a>(
        &'a self,
        context: chigiri::FieldContext<'a, ()>,
        _: generated::MutationRenameArgs,
    ) -> chigiri::FieldResult<String> {
        Query { fail: false }.greeting(context).await
    }
}

fn build(fail: bool) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query { fail })
        .with_mutation(MutationFields)
        .with_item(ItemFields)
        .build()
}
