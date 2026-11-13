#![deny(warnings)]

pub use ::generated;

pub struct Types;

impl generated::SchemaTypes for Types {}

pub struct QueryFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn ok<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::QueryOkArgs,
    ) -> chigiri::FieldResult<bool> {
        Ok(true)
    }
}

pub fn resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(QueryFields)
        .build()
}
