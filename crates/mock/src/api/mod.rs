//! Model-operation contract shared by host registration, validation, and Luau reference types.

pub(crate) mod constraints;
pub(crate) mod consumers;
pub(crate) mod context;
pub(crate) mod examples;
pub(crate) mod hook_rules;
pub(crate) mod hook_values;
pub(crate) mod hooks;
mod methods;
pub(crate) mod model_values;
mod options;
pub(crate) mod query;
pub(crate) mod runtime;
pub(crate) mod semantics;
pub(crate) mod shape;
pub(crate) mod subscriptions;
#[cfg(test)]
mod tests;

pub(crate) use methods::{METHODS, Method, MethodSpec, validate};
pub(crate) use options::{
    COUNT_OPTIONS, FIND_LIMIT, FIND_OFFSET, FIND_OPTIONS, IntegerOption, OptionSpec, OptionType,
};

pub(crate) const HOST_CALL_LIMIT: usize = 256;
pub(crate) const WORK_LIMIT: usize =
    crate::limits::request::policy(crate::limits::request::LimitKind::Work).hard;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Effect {
    Read,
    Write,
}

impl Effect {
    pub fn allowed(self, writable: bool) -> bool {
        self == Self::Read || writable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TypeExpr {
    FindOptions,
    CountOptions,
    TargetOptions,
    Relation,
    CreateInput,
    UpdateInput,
    Record,
    Records,
    Number,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArgumentSpec {
    pub name: &'static str,
    pub ty: TypeExpr,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuleSpec {
    pub id: &'static str,
    pub description: &'static str,
    pub failure: FailureClass,
    pub example: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureClass {
    Query,
    Storage,
    Relation,
    Capability,
    Resource,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelEligibility {
    All,
    Relations,
}

impl ModelEligibility {
    pub fn allows(self, ir: &crate::ir::MockIr, model: crate::ir::ModelId) -> bool {
        ir.models().get(model.0).is_some()
            && (self == Self::All || ir.relation_fields(model).next().is_some())
    }
}
