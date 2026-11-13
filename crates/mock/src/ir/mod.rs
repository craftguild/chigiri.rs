//! Resolve GraphQL names to stable IDs and validate shared storage and resolver semantics.

mod constraints;
mod cycles;
mod graph;
mod meaning;
mod plans;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_constraints;
#[cfg(test)]
mod tests_patterns;
mod types;

pub use constraints::{Constraints, Format, Pattern, Representation, ScalarSpec};
pub use cycles::RequiredReferenceCycle;
pub use graph::SchemaGraph;
pub use meaning::MockIr;
pub use plans::{
    FieldPlan, KeyId, KeySpec, Model, ModelId, Relation, RelationId, RelationStorage, ScriptId,
    ScriptPlan, ScriptRole, Slot, SlotId,
};
pub use types::{FieldId, FieldNode, InputId, InputNode, TypeId, TypeNode, TypeRef, Wrapper};
