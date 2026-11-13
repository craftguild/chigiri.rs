//! Application domain types for the synthetic adoption example.

use crate::graphql::generated;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessLevel {
    Public,
    Private,
}

#[derive(Clone)]
pub struct Viewer {
    pub id: chigiri::Id,
    pub access: AccessLevel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: chigiri::Id,
    pub title: String,
}

pub struct Types;

impl generated::SchemaTypes for Types {
    type AccessLevel = AccessLevel;
    type Viewer = Viewer;
    type Item = Item;

    fn access_level_from_graphql(
        value: generated::AccessLevel,
    ) -> Result<Self::AccessLevel, chigiri::EnumBindingError> {
        Ok(match value {
            generated::AccessLevel::Public => AccessLevel::Public,
            generated::AccessLevel::Private => AccessLevel::Private,
        })
    }

    fn access_level_to_graphql(
        value: &Self::AccessLevel,
    ) -> Result<generated::AccessLevel, chigiri::EnumBindingError> {
        Ok(match value {
            AccessLevel::Public => generated::AccessLevel::Public,
            AccessLevel::Private => generated::AccessLevel::Private,
        })
    }
}
