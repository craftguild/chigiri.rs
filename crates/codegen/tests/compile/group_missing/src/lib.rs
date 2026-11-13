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

pub fn missing_group_is_an_error() {
    let _ =
        generated::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>)
            .build();
}
