//! Stored model-value shapes and Luau presence/null semantics.

pub(crate) const TYPENAME: &str = "__typename";

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Record,
    Create,
    Update,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub include_key: bool,
    pub omission: Omission,
    pub top_level_null: NullAllowance,
    pub validate_storage_constraints: bool,
    pub required_model_typename: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Omission {
    NullableSlots,
    AllSlots,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NullAllowance {
    StorageNullability,
    AllSlots,
}

impl Shape {
    pub fn policy(self) -> Policy {
        Policy {
            include_key: !matches!(self, Self::Update),
            omission: if matches!(self, Self::Update) {
                Omission::AllSlots
            } else {
                Omission::NullableSlots
            },
            top_level_null: if matches!(self, Self::Update) {
                NullAllowance::AllSlots
            } else {
                NullAllowance::StorageNullability
            },
            validate_storage_constraints: true,
            required_model_typename: matches!(self, Self::Record),
        }
    }
}

impl Policy {
    pub fn optional(self, storage_nullable: bool) -> bool {
        storage_nullable || self.omission == Omission::AllSlots
    }
    pub fn extra_null(self, storage_nullable: bool) -> bool {
        !storage_nullable && self.top_level_null == NullAllowance::AllSlots
    }
}
