//! Verify resolver panics remain application failures.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::generated;
use super::support::{Item, MutationFields, NeverDeadline, QueryFields, Types, block_on};

struct PanickingItem;

impl generated::ItemResolver<(), Types, chigiri::FieldError> for PanickingItem {
    async fn id<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Item,
    ) -> chigiri::FieldResult<chigiri::Id> {
        panic!("resolver panic sentinel")
    }

    async fn name<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        item: &'a Item,
    ) -> chigiri::FieldResult<String> {
        Ok(item.name.clone())
    }
}

#[test]
fn resolver_panics_propagate_to_the_application_boundary() {
    let full_endpoint = chigiri::Endpoint::builder(
        generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
            .with_query(QueryFields)
            .with_mutation(MutationFields)
            .with_item(PanickingItem)
            .build(),
    )
    .without_context()
    .build()
    .unwrap();

    {
        let endpoint = &full_endpoint;
        let result = catch_unwind(AssertUnwindSafe(|| {
            block_on(endpoint.execute(
                chigiri::GraphQlRequest::new("{ item { id } }"),
                (),
                &NeverDeadline,
            ))
        }));
        assert!(
            result.is_err(),
            "a resolver panic must not become a GraphQL response"
        );
    }
}
