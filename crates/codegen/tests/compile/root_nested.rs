#![deny(warnings)]

pub use ::generated;

pub struct Types;

pub struct QueryValue {
    pub value: String,
}

impl generated::SchemaTypes for Types {
    type Query = QueryValue;
}

pub struct QueryFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn child<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        parent: Option<&'a QueryValue>,
    ) -> chigiri::FieldResult<Option<QueryValue>> {
        Ok(parent.is_none().then(|| QueryValue {
            value: "nested".to_owned(),
        }))
    }

    async fn value<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        parent: Option<&'a QueryValue>,
    ) -> chigiri::FieldResult<String> {
        Ok(parent.map_or_else(|| "root".to_owned(), |value| value.value.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use chigiri::__private::{ArgumentValues, ResolvedObject, ResolvedValue, ResolverDispatch};

    use super::{QueryFields, QueryValue, Types, generated};

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

    fn resolve(
        resolvers: &generated::Resolvers<(), Types>,
        field: &'static str,
        parent: Option<&ResolvedObject>,
    ) -> ResolvedValue {
        block_on(resolvers.resolve(&(), "Query", field, parent, ArgumentValues::new(vec![])))
            .unwrap()
    }

    #[test]
    fn passes_none_for_root_and_some_for_nested_values_in_the_same_resolver_group() {
        let resolvers =
            generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
                .with_query(QueryFields)
                .build();
        let root = resolve(&resolvers, "value", None);
        assert!(
            matches!(root, ResolvedValue::Leaf(chigiri::Value::String(value)) if value == "root")
        );

        let child = resolve(&resolvers, "child", None);
        let ResolvedValue::Object(child) = child else {
            panic!("expected a query object.");
        };
        assert_eq!(child.type_name(), "Query");

        let parent = ResolvedObject::new(
            "Query",
            QueryValue {
                value: "nested".to_owned(),
            },
        );
        let nested = resolve(&resolvers, "value", Some(&parent));
        assert!(
            matches!(nested, ResolvedValue::Leaf(chigiri::Value::String(value)) if value == "nested")
        );
    }
}
