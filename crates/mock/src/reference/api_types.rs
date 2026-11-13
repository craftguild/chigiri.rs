//! Shared API wire types with closed enum and field sets for JSON decoding.

use crate::api::query::{Eligibility, OperandType};
use crate::api::{Effect, FailureClass, ModelEligibility, TypeExpr};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MethodDoc {
    pub id: String,
    pub name: String,
    pub receiver: String,
    pub arity_including_receiver: usize,
    pub effect: Effect,
    pub model_eligibility: ModelEligibility,
    pub arguments: Vec<ArgumentDoc>,
    pub returns: TypeExpr,
    pub rule_ids: Vec<String>,
    pub example_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArgumentDoc {
    pub name: String,
    pub required: bool,
    pub value_type: TypeExpr,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OptionDoc {
    pub id: String,
    pub method_id: String,
    pub name: String,
    pub value_type: OptionKind,
    pub required: bool,
    pub nullable: bool,
    pub default: Option<usize>,
    pub maximum: Option<usize>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OptionKind {
    Predicate,
    SortKeys,
    NonnegativeInteger,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OperatorDoc {
    pub id: String,
    pub name: String,
    pub eligibility: Eligibility,
    pub operand: OperandType,
    pub rule_ids: Vec<String>,
    pub example_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuleDoc {
    pub id: String,
    pub description: String,
    pub failure_class: FailureClass,
    pub example_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExampleDoc {
    pub id: String,
    pub title: String,
    pub origin: ExampleOrigin,
    pub scope: Effect,
    pub luau: String,
    pub rule_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExampleOrigin {
    GenericIllustration,
    SchemaSpecific,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LimitDoc {
    pub id: String,
    pub value: usize,
    pub unit: LimitUnit,
    pub shared_scope: String,
    pub description: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LimitUnit {
    Bytes,
    Nodes,
    Items,
    Calls,
    Work,
    Milliseconds,
    Depth,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextDoc {
    pub id: String,
    pub type_id: String,
    pub scope_value: String,
    pub effect: Effect,
    pub method_ids: Vec<String>,
    pub rule_ids: Vec<String>,
}
