pub use ::generated;

struct Types;
struct DateTime;
struct User;
struct Team;

impl chigiri::Scalar for DateTime {
    fn parse(_value: chigiri::Value) -> Result<Self, chigiri::ScalarCoercionError> {
        Ok(Self)
    }

    fn serialize(&self) -> chigiri::FieldResult<chigiri::Value> {
        Ok(chigiri::Value::String("fixture".to_owned()))
    }
}

impl generated::SchemaTypes for Types {
    type DateTime = DateTime;
    type User = User;
    type Team = Team;
}

struct QueryFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn user<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::QueryUserArgs,
    ) -> chigiri::FieldResult<Option<User>> {
        Ok(None)
    }

    async fn search<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, ()>,
        _args: generated::QuerySearchArgs,
    ) -> chigiri::FieldResult<Vec<generated::SearchResult<Types>>> {
        Ok(Vec::new())
    }
}
