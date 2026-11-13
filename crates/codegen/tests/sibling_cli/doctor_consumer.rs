//! Test doctor against an external consumer crate with application-local resolver implementations.

pub struct Types;
impl ::contract::SchemaTypes for Types {}

pub struct Query;
impl ::contract::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn value<'a>(
        &'a self,
        _ctx: ::chigiri::FieldContext<'a, ()>,
    ) -> ::chigiri::FieldResult<Option<String>> {
        Ok(None)
    }
}

pub fn resolvers() -> ::contract::Resolvers<(), Types> {
    ::contract::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query)
        .build()
}
