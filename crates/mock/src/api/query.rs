//! Query operators shared by value validation, type declarations, and reference output.

use super::{FailureClass, RuleSpec};
use crate::ir::{MockIr, Representation, Slot, TypeId};
use chigiri::core::schema::TypeKind;

pub(crate) const PREDICATE_DEPTH: usize = 8;
pub(crate) const PREDICATE_NODES: usize = 128;
pub(crate) const IN_ITEMS: usize = 100;
pub(crate) const OPERAND_BYTES: usize = 4096;
pub(crate) const SORT_KEYS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Ascending,
    Descending,
}

pub(crate) const DIRECTIONS: &[(&str, Direction)] = &[
    ("asc", Direction::Ascending),
    ("desc", Direction::Descending),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    String,
    Id,
    Enum(TypeId),
    Boolean,
    Int,
    Integer,
    Number,
}

impl ScalarKind {
    pub fn numeric(self) -> bool {
        matches!(self, Self::Int | Self::Integer | Self::Number)
    }
}

pub(crate) fn scalar(ir: &MockIr, slot: &Slot) -> Option<ScalarKind> {
    if slot.ty.list_depth() != 0 {
        return None;
    }
    let node = &ir.graph().types()[slot.ty.named.0];
    if node.kind == TypeKind::Enum {
        return Some(ScalarKind::Enum(slot.ty.named));
    }
    if node.kind != TypeKind::Scalar {
        return None;
    }
    if let Some(spec) = &node.scalar {
        return match spec.representation {
            Representation::String => Some(ScalarKind::String),
            Representation::Boolean => Some(ScalarKind::Boolean),
            Representation::Integer => Some(ScalarKind::Integer),
            Representation::Number => Some(ScalarKind::Number),
            _ => None,
        };
    }
    match node.name.as_ref() {
        "String" => Some(ScalarKind::String),
        "ID" => Some(ScalarKind::Id),
        "Int" => Some(ScalarKind::Int),
        "Float" => Some(ScalarKind::Number),
        "Boolean" => Some(ScalarKind::Boolean),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    Eq,
    Ne,
    In,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
    IsNull,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperandType {
    Scalar,
    List,
    Boolean,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Eligibility {
    Simple,
    Numeric,
    String,
}

impl Eligibility {
    pub fn allows(self, kind: ScalarKind) -> bool {
        match self {
            Self::Simple => true,
            Self::Numeric => kind.numeric(),
            Self::String => kind == ScalarKind::String,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OperatorSpec {
    pub name: &'static str,
    pub operator: Operator,
    pub eligibility: Eligibility,
    pub operand: OperandType,
    pub rule: RuleSpec,
}

pub(crate) const OPERATORS: &[OperatorSpec] = &[
    OperatorSpec {
        name: "eq",
        operator: Operator::Eq,
        rule: RuleSpec {
            id: "predicate.eq",
            description: "True when two non-null values are equal. Strings, IDs, and enums use exact matching; numbers use exact numeric comparison.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Simple,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "ne",
        operator: Operator::Ne,
        rule: RuleSpec {
            id: "predicate.ne",
            description: "True when two non-null values are not equal.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Simple,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "in",
        operator: Operator::In,
        rule: RuleSpec {
            id: "predicate.in",
            description: "True when the non-null field value equals one of the values in the operand list.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Simple,
        operand: OperandType::List,
    },
    OperatorSpec {
        name: "lt",
        operator: Operator::Lt,
        rule: RuleSpec {
            id: "predicate.lt",
            description: "True when the numeric field value is less than the operand.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Numeric,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "lte",
        operator: Operator::Lte,
        rule: RuleSpec {
            id: "predicate.lte",
            description: "True when the numeric field value is less than or equal to the operand.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Numeric,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "gt",
        operator: Operator::Gt,
        rule: RuleSpec {
            id: "predicate.gt",
            description: "True when the numeric field value is greater than the operand.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Numeric,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "gte",
        operator: Operator::Gte,
        rule: RuleSpec {
            id: "predicate.gte",
            description: "True when the numeric field value is greater than or equal to the operand.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Numeric,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "contains",
        operator: Operator::Contains,
        rule: RuleSpec {
            id: "predicate.contains",
            description: "True when the string field contains the operand as a case-sensitive substring.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::String,
        operand: OperandType::Scalar,
    },
    OperatorSpec {
        name: "isNull",
        operator: Operator::IsNull,
        rule: RuleSpec {
            id: "predicate.isnull",
            description: "With operand true, match null or omitted fields; with operand false, match non-null fields.",
            failure: FailureClass::Query,
            example: "query.operators",
        },
        eligibility: Eligibility::Simple,
        operand: OperandType::Boolean,
    },
];
