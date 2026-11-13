//! Builds schema-derived expected types for precise Luau mismatch diagnostics.

use std::sync::Arc;

use full_moon::ast::Expression;
use full_moon::tokenizer::{Symbol, TokenType};

use super::names;
use crate::api::query::{DIRECTIONS, OPERATORS, OperandType};
use crate::ir::{MockIr, Representation, ScriptId, TypeRef, Wrapper};

#[derive(Clone, Debug)]
pub(super) struct ExpectedSite {
    pub(super) member: String,
    pub(super) expected: String,
    pub(super) hint: &'static str,
    kind: ExpectedKind,
}

pub(super) struct FieldSite {
    pub(super) alias: Option<String>,
    pub(super) value: ExpectedSite,
    pub(super) item: Option<ExpectedSite>,
}

#[derive(Clone, Debug)]
enum ExpectedKind {
    String,
    Number,
    Boolean,
    List,
    Enum(Vec<Arc<str>>),
    Direction,
    Record,
    Unknown,
}

impl ExpectedSite {
    pub(super) fn accepts(&self, expression: &Expression) -> Option<bool> {
        self.kind.accepts(expression)
    }
}

pub(super) fn field(
    ir: &MockIr,
    model_name: &str,
    field: &str,
    operator: &str,
) -> Option<FieldSite> {
    let operator = OPERATORS
        .iter()
        .find(|candidate| candidate.name == operator)?;
    let ty = ir.graph().type_id(model_name)?;
    let model = ir.model_for(ty)?;
    let slot = &ir.slots()[ir.models()[model.0].slots.get(field)?.0];
    let kind = crate::api::query::scalar(ir, slot)?;
    if !operator.eligibility.allows(kind) {
        return None;
    }
    let named = slot.ty.named;
    let node = &ir.graph().types()[named.0];
    let member = format!("{model_name}.{field}");
    let (alias, scalar) = if node.kind == chigiri::core::schema::TypeKind::Enum {
        let mut alternatives = node.enum_values.iter().take(8).cloned().collect::<Vec<_>>();
        if node.enum_values.len() > alternatives.len() {
            alternatives.push("...".into());
        }
        (
            Some(names::ty(ir, named, "Value")),
            ExpectedSite {
                member: member.clone(),
                expected: format!(
                    "enum {} for {member} ({})",
                    node.name,
                    alternatives.join(" | ")
                ),
                hint: "Use one of the enum alternatives shown in expected.",
                kind: ExpectedKind::Enum(node.enum_values.clone()),
            },
        )
    } else {
        (
            None,
            ExpectedSite {
                member: member.clone(),
                expected: format!("{} value for {member}", node.name),
                hint: "Use a value compatible with the field type shown in expected.",
                kind: named_kind(ir, named),
            },
        )
    };
    match operator.operand {
        OperandType::Scalar => Some(FieldSite {
            alias,
            value: scalar,
            item: None,
        }),
        OperandType::List => Some(FieldSite {
            alias,
            value: ExpectedSite {
                member,
                expected: format!("list of {} values for {model_name}.{field}", node.name),
                hint: "Use a list of values compatible with the field type shown in expected.",
                kind: ExpectedKind::List,
            },
            item: Some(scalar),
        }),
        OperandType::Boolean => Some(FieldSite {
            alias: None,
            value: ExpectedSite {
                member,
                expected: format!("boolean isNull operand for {model_name}.{field}"),
                hint: "Use true or false for an isNull condition.",
                kind: ExpectedKind::Boolean,
            },
            item: None,
        }),
    }
}

pub(super) fn order_direction(model: &str) -> ExpectedSite {
    ExpectedSite {
        member: format!("{model}.orderBy.direction"),
        expected: format!("sort direction for {model}.orderBy.direction (asc | desc)"),
        hint: "Use asc or desc.",
        kind: ExpectedKind::Direction,
    }
}

pub(super) fn integer_option(model: &str, method: &str, name: &str) -> ExpectedSite {
    let member = format!("{model}.{method}.{name}");
    ExpectedSite {
        expected: format!("nonnegative integer for {member}"),
        member,
        hint: "Use a nonnegative integer within the supported range.",
        kind: ExpectedKind::Number,
    }
}

pub(super) fn local(annotation: &str) -> Option<ExpectedSite> {
    simple(
        annotation,
        "local binding",
        "Assign a value compatible with the declared type.",
    )
}

pub(super) fn function_return(ir: &MockIr, annotation: &str) -> Option<ExpectedSite> {
    for (index, plan) in ir.scripts().iter().enumerate() {
        let id = ScriptId(index);
        if names::resolver(ir, id, "Result") != annotation {
            continue;
        }
        let field = &ir.graph().fields()[plan.field.0];
        let parent = &ir.graph().types()[field.parent.0];
        let member = format!("{}.{}", parent.name, field.name);
        return Some(ExpectedSite {
            expected: format!("{} result for {member}", reference(ir, &field.ty)),
            member,
            hint: "Return a value compatible with the resolver result shown in expected.",
            kind: reference_kind(ir, &field.ty),
        });
    }
    simple(
        annotation,
        "function return",
        "Return a value compatible with the declared type.",
    )
}

fn simple(annotation: &str, member: &str, hint: &'static str) -> Option<ExpectedSite> {
    let kind = match annotation {
        "number" => ExpectedKind::Number,
        "string" => ExpectedKind::String,
        "boolean" => ExpectedKind::Boolean,
        _ => return None,
    };
    Some(ExpectedSite {
        member: member.into(),
        expected: annotation.into(),
        hint,
        kind,
    })
}

fn reference(ir: &MockIr, reference: &TypeRef) -> String {
    let mut out = String::new();
    for wrapper in reference.wrappers.iter() {
        if *wrapper == Wrapper::List {
            out.push('[');
        }
    }
    out.push_str(&ir.graph().types()[reference.named.0].name);
    for wrapper in reference.wrappers.iter().rev() {
        out.push(match wrapper {
            Wrapper::NonNull => '!',
            Wrapper::List => ']',
        });
    }
    out
}

impl ExpectedKind {
    fn accepts(&self, expression: &Expression) -> Option<bool> {
        match self {
            Self::String => Some(matches!(expression, Expression::String(_))),
            Self::Number => Some(matches!(expression, Expression::Number(_))),
            Self::Boolean => Some(boolean_literal(expression)),
            Self::List => Some(matches!(expression, Expression::TableConstructor(_))),
            Self::Enum(values) => {
                if !matches!(expression, Expression::String(_)) {
                    return Some(false);
                }
                let (_, value) = super::literal_string::decoded(expression)?;
                Some(values.iter().any(|candidate| candidate.as_ref() == value))
            }
            Self::Direction => {
                if !matches!(expression, Expression::String(_)) {
                    return Some(false);
                }
                let (_, value) = super::literal_string::decoded(expression)?;
                Some(DIRECTIONS.iter().any(|(direction, _)| *direction == value))
            }
            Self::Record => Some(matches!(expression, Expression::TableConstructor(_))),
            Self::Unknown => None,
        }
    }
}

fn reference_kind(ir: &MockIr, reference: &TypeRef) -> ExpectedKind {
    if reference.wrappers.contains(&Wrapper::List) {
        ExpectedKind::List
    } else {
        named_kind(ir, reference.named)
    }
}

fn named_kind(ir: &MockIr, named: crate::ir::TypeId) -> ExpectedKind {
    let node = &ir.graph().types()[named.0];
    if node.kind == chigiri::core::schema::TypeKind::Enum {
        return ExpectedKind::Enum(node.enum_values.clone());
    }
    if node.kind != chigiri::core::schema::TypeKind::Scalar {
        return ExpectedKind::Record;
    }
    if let Some(spec) = &node.scalar {
        return match spec.representation {
            Representation::String => ExpectedKind::String,
            Representation::Boolean => ExpectedKind::Boolean,
            Representation::Integer | Representation::Number => ExpectedKind::Number,
            Representation::Object => ExpectedKind::Record,
            Representation::Array => ExpectedKind::List,
            Representation::Json => ExpectedKind::Unknown,
        };
    }
    match node.name.as_ref() {
        "String" | "ID" => ExpectedKind::String,
        "Int" | "Float" => ExpectedKind::Number,
        "Boolean" => ExpectedKind::Boolean,
        _ => ExpectedKind::Unknown,
    }
}

fn boolean_literal(expression: &Expression) -> bool {
    let Expression::Symbol(token) = expression else {
        return false;
    };
    matches!(
        token.token().token_type(),
        TokenType::Symbol {
            symbol: Symbol::True | Symbol::False
        }
    )
}
