//! Describe storage models, relations, and field plans with resolved IDs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{Constraints, FieldId, TypeId, TypeRef};

/// Model index in the IR.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelId(pub usize);
/// Storage-slot index across all models.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SlotId(pub usize);
/// Key-specification index in the IR.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct KeyId(pub usize);
/// Relation index in the IR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelationId(pub usize);
/// Script-plan index in the IR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptId(pub usize);

/// Storage model and its field-to-slot mapping.
#[derive(Debug)]
pub struct Model {
    /// Concrete GraphQL object type backing the model.
    pub ty: TypeId,
    /// Optional `id` field used as the model key.
    pub key: Option<FieldId>,
    /// Whether an omitted non-null `ID` or `String` key receives a generated ULID.
    pub auto_id: bool,
    /// Stored field name to slot index.
    pub slots: BTreeMap<Arc<str>, SlotId>,
    /// Non-null slots whose presence must be validated.
    pub required_slots: Vec<SlotId>,
    /// Identity and reference-key components that updates cannot change.
    pub immutable_slots: BTreeSet<SlotId>,
}

/// Ordered slots that form one reference key.
#[derive(Debug)]
pub struct KeySpec {
    /// Model that owns the key.
    pub model: ModelId,
    /// Key slots in declaration order.
    pub slots: Vec<SlotId>,
}

/// Storage strategy for foreign-key and managed many-to-many relations.
#[derive(Debug)]
pub enum RelationStorage {
    /// Explicit and hidden foreign keys use model storage slots.
    ForeignKey {
        /// Foreign-key slots in declaration order.
        slots: Vec<SlotId>,
        /// Resolved referenced key.
        key: KeyId,
    },
    /// Store an implicit many-to-many relation between the declared lists on both sides.
    Implicit {
        /// Non-null identity key on the owner side.
        owner_key: KeyId,
        /// Non-null identity key on the target side.
        target_key: KeyId,
    },
}

/// One stored value or owner relation with its target key type and original wrappers.
#[derive(Debug)]
pub struct Slot {
    /// Owning model.
    pub model: ModelId,
    /// GraphQL field name or synthetic relation-storage name.
    pub name: Arc<str>,
    /// Stored value type.
    pub ty: TypeRef,
    /// The original field.
    pub field: FieldId,
    /// Constraints on the stored value.
    pub constraints: Constraints,
}

/// A relation stores identity in the owner direction and derives inverse lookups from it.
#[derive(Debug)]
pub struct Relation {
    /// Owner-side field.
    pub field: FieldId,
    /// Owner model.
    pub owner: ModelId,
    /// Target model.
    pub target: ModelId,
    /// FK component or implicit key group.
    pub storage: RelationStorage,
    /// Optional relation name shared by both corresponding fields.
    pub name: Option<Arc<str>>,
    /// Inverse fields that refer to the same owner relation.
    pub inverses: Vec<FieldId>,
    /// Whether each target identity may be referenced by at most one owner.
    pub unique: bool,
    /// The minimum number of reference records required for each target in the final state.
    pub min_refs: usize,
    /// Maximum owner references per target imposed by inverse-field constraints.
    pub max_refs: Option<usize>,
}

/// The interface declaration itself does not dispatch.
#[derive(Clone, Debug)]
pub enum FieldPlan {
    /// Read the value from a model record slot.
    Stored(SlotId),
    /// Read the value from a DTO.
    Value,
    /// Resolve a relation from its owner-side stored identity.
    Owner(RelationId),
    /// Resolve the inverse side of a relation.
    Inverse { relation: RelationId },
    /// Invoke a validated Luau script.
    Script(ScriptId),
}

/// Closed resolver role derived from the field owner and the root's nested use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptRole {
    /// Query-root resolver, optionally also callable through an ordinary nested parent.
    QueryRoot { root_as_nested: bool },
    /// Mutation-root resolver, optionally also callable through an ordinary nested parent.
    MutationRoot { root_as_nested: bool },
    /// Subscription source initializer, plus an ordinary resolver only when nested.
    SubscriptionRoot { root_as_nested: bool },
    /// Ordinary non-root object resolver.
    Object,
}

impl ScriptRole {
    /// Whether this coordinate belongs to the mutation root.
    pub const fn is_mutation_root(self) -> bool {
        matches!(self, Self::MutationRoot { .. })
    }

    /// Whether this root also needs ordinary nested-object dispatch.
    pub const fn root_as_nested(self) -> bool {
        match self {
            Self::QueryRoot { root_as_nested }
            | Self::MutationRoot { root_as_nested }
            | Self::SubscriptionRoot { root_as_nested } => root_as_nested,
            Self::Object => false,
        }
    }

    /// Whether this coordinate is a subscription source initializer.
    pub const fn is_subscription_root(self) -> bool {
        matches!(self, Self::SubscriptionRoot { .. })
    }
}

/// Link one scripted field to its closed execution role.
#[derive(Debug)]
pub struct ScriptPlan {
    /// Field identified by this script coordinate.
    pub field: FieldId,
    /// Root or object execution role.
    pub role: ScriptRole,
}
