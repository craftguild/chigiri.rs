pub use ::generated;

pub struct Types;
pub struct DateTime;
pub struct User;
pub struct Team;
pub struct Candidate;

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
