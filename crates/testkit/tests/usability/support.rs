//! Small generated schema shared by usability acceptance tests and execution support.

use std::future::Future;
use std::task::{Context, Poll, Waker};

use chigiri::spi::{ExecutableSchema, ExecutableSchemaBuilder};

use super::generated;

pub(crate) struct Types;

#[derive(Clone)]
pub(crate) struct Item {
    pub(crate) id: chigiri::Id,
    pub(crate) name: String,
}

impl generated::SchemaTypes for Types {
    type Item = Item;
}

pub(crate) struct QueryFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("hello".to_owned())
    }

    async fn item<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> chigiri::FieldResult<Item> {
        Ok(Item {
            id: "item-1".into(),
            name: "Chigiri".to_owned(),
        })
    }
}

pub(crate) struct MutationFields;

impl generated::MutationResolver<(), Types, chigiri::FieldError> for MutationFields {
    async fn rename<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        args: generated::MutationRenameArgs,
    ) -> chigiri::FieldResult<String> {
        Ok(args.name)
    }
}

pub(crate) struct ItemFields;

impl generated::ItemResolver<(), Types, chigiri::FieldError> for ItemFields {
    async fn id<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        item: &'a Item,
    ) -> chigiri::FieldResult<chigiri::Id> {
        Ok(item.id.clone())
    }
    async fn name<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        item: &'a Item,
    ) -> chigiri::FieldResult<String> {
        Ok(item.name.clone())
    }
}

pub(crate) fn resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .with_item(ItemFields)
        .build()
}

pub(crate) fn schema_builder() -> ExecutableSchemaBuilder<()> {
    ExecutableSchema::builder(&generated::SCHEMA_DESCRIPTOR, resolvers())
}

pub(crate) fn schema() -> ExecutableSchema<()> {
    schema_builder().build().unwrap()
}

pub(crate) fn endpoint() -> chigiri::Endpoint<()> {
    chigiri::Endpoint::builder(resolvers())
        .without_context()
        .build()
        .unwrap()
}

pub(crate) fn execute(
    endpoint: &chigiri::Endpoint<()>,
    document: &str,
) -> chigiri::GraphQlResponse {
    block_on(endpoint.execute(chigiri::GraphQlRequest::new(document), (), &NeverDeadline))
}

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

pub(crate) struct NeverDeadline;

impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
