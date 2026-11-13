//! Validate declared request state and provide immutable snapshots with atomic field patches.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::{Object, Value};
use serde::Serialize;

use super::{HookSlot, state_schema, state_value};
use crate::value::{ValueBudget, ValueError};

/// Closed state-field type declarations; types are never inferred from values.
pub type StateFields = BTreeMap<String, StateType>;

/// Serializable request-state type description with no Luau type strings or VM handles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateType {
    /// Bounded UTF-8 string.
    String,
    /// Boolean value.
    Boolean,
    /// Finite integer within ±(2^53 − 1).
    Integer,
    /// Finite number.
    Number,
    /// Object containing only declared fields.
    Object {
        /// The nested object is a closed field set.
        fields: StateFields,
    },
    /// Ordered list.
    List {
        /// Shared type of every list item.
        of: Box<StateType>,
    },
    /// Explicit null or a value of the nested type.
    Nullable {
        /// Type other than null.
        of: Box<StateType>,
    },
}

impl StateType {
    /// Whether a missing initial field may be normalized to null.
    pub const fn nullable(&self) -> bool {
        matches!(self, Self::Nullable { .. })
    }
}

/// Request-state declaration and initial value, independent of the GraphQL schema digest.
#[derive(Clone, Debug, Default)]
pub struct StateContract {
    fields: Arc<StateFields>,
    initial: Arc<Object>,
}

impl Serialize for StateContract {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Initial<'a> {
            fields: &'a StateFields,
            #[serde(serialize_with = "crate::value::serialize::object")]
            initial: &'a Object,
        }
        Initial {
            fields: self.fields(),
            initial: self.initial(),
        }
        .serialize(serializer)
    }
}

impl StateContract {
    /// Parse and validate a closed declaration and its initial value with one shared budget.
    pub fn parse(
        fields: &Value,
        initial: &Value,
        budget: &mut ValueBudget,
    ) -> Result<Self, ValueError> {
        let fields = state_schema::fields(fields, budget, 1)?;
        Self::checked(fields, initial, budget)
    }

    /// Validate a decoded declaration and construct its contract from the initial value.
    pub fn checked(
        fields: StateFields,
        initial: &Value,
        budget: &mut ValueBudget,
    ) -> Result<Self, ValueError> {
        state_schema::validate(&fields, budget, 1)?;
        let initial = state_value::object(&fields, initial, None, budget, 1)?;
        Ok(Self {
            fields: Arc::new(fields),
            initial: Arc::new(initial),
        })
    }

    /// Return the declaration shared by type generation and runtime decoding.
    pub fn fields(&self) -> &StateFields {
        &self.fields
    }

    /// Return the initial value used for export, not any live request state.
    pub fn initial(&self) -> &Object {
        &self.initial
    }

    /// Share the immutable initial value until an update creates a new snapshot.
    pub fn instantiate(&self) -> RequestState {
        RequestState {
            fields: self.fields.clone(),
            value: self.initial.clone(),
        }
    }

    pub(crate) fn owns(&self, state: &RequestState) -> bool {
        Arc::ptr_eq(&self.fields, &state.fields)
    }
}

/// Immutable state snapshot for one request.
#[derive(Clone, Debug)]
pub struct RequestState {
    fields: Arc<StateFields>,
    value: Arc<Object>,
}

impl RequestState {
    /// Return the declaration used by runtime decoding, including empty-table shape information.
    pub(crate) fn fields(&self) -> &StateFields {
        &self.fields
    }

    /// Return the current immutable state snapshot.
    pub fn snapshot(&self) -> Arc<Object> {
        self.value.clone()
    }

    /// Replace the snapshot only after the complete patch validates.
    pub fn apply_patch(
        &mut self,
        slot: HookSlot,
        patch: &Value,
        budget: &mut ValueBudget,
    ) -> Result<(), ValueError> {
        if !slot.allows_patch() {
            return Err(ValueError("hook cannot update state"));
        }
        let next = state_value::object(&self.fields, patch, Some(&self.value), budget, 1)?;
        self.value = Arc::new(next);
        Ok(())
    }
}
