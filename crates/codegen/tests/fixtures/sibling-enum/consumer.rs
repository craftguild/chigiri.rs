//! Connects external domain types to application-local types.

use std::future::Future;
use std::task::{Context, Poll, Waker};

pub struct Types;

impl contract::SchemaTypes for Types {
    type AdapterKind = external_domain::AdapterKind;
    type Item = external_domain::Item;

    fn adapter_kind_from_graphql(
        value: contract::AdapterKind,
    ) -> Result<Self::AdapterKind, chigiri::EnumBindingError> {
        match value {
            contract::AdapterKind::V3 => Ok(external_domain::AdapterKind::V3),
            contract::AdapterKind::V4 => Ok(external_domain::AdapterKind::V4),
        }
    }

    fn adapter_kind_to_graphql(
        value: &Self::AdapterKind,
    ) -> Result<contract::AdapterKind, chigiri::EnumBindingError> {
        match value {
            external_domain::AdapterKind::V3 => Ok(contract::AdapterKind::V3),
            external_domain::AdapterKind::V4 => Ok(contract::AdapterKind::V4),
        }
    }
}

pub struct Query;

impl contract::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn echo<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        args: contract::QueryEchoArgs<Types>,
    ) -> chigiri::FieldResult<external_domain::AdapterKind> {
        Ok(args.kind)
    }

    async fn item<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<external_domain::Item> {
        Ok(external_domain::Item {
            kind: external_domain::AdapterKind::V4,
        })
    }
}

pub struct ItemFields;

impl contract::ItemResolver<(), Types, chigiri::FieldError> for ItemFields {
    async fn kind<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        item: &'a external_domain::Item,
    ) -> chigiri::FieldResult<external_domain::AdapterKind> {
        Ok(item.kind)
    }
}

pub fn resolvers() -> contract::Resolvers<(), Types> {
    contract::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query)
        .with_item(ItemFields)
        .build()
}

#[test]
fn converts_the_external_enum_in_both_directions() {
    use contract::SchemaTypes as _;
    assert_eq!(
        Types::adapter_kind_from_graphql(contract::AdapterKind::V3).unwrap(),
        external_domain::AdapterKind::V3
    );
    assert_eq!(
        Types::adapter_kind_to_graphql(&external_domain::AdapterKind::V4).unwrap(),
        contract::AdapterKind::V4
    );
    let _ = resolvers();
    contract::generator().validate().unwrap();
    assert_eq!(chigiri::GENERATED_ABI_VERSION, 8);
}

#[test]
fn generated_traits_execute_the_expected_graphql_behavior() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(
        chigiri::GraphQlRequest::new("{ echo(kind: V3) item { kind } }"),
        (),
        &NeverDeadline,
    ));
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let Some(chigiri::Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    assert_eq!(data.get("echo"), Some(&chigiri::Value::from("V3")));
    let Some(chigiri::Value::Object(item)) = data.get("item") else {
        panic!("expected item data");
    };
    assert_eq!(item.get("kind"), Some(&chigiri::Value::from("V4")));
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
