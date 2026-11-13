//! Copy shared API definitions into wire types with validated descriptions and use-case ID references.

use super::{ReferenceDocument, ReferenceError, api_types::*, budget::Budget};
use crate::api::{self, Effect, OptionType};

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    for method in api::METHODS {
        budget.push(
            &mut doc.methods,
            MethodDoc {
                id: method.id.into(),
                name: method.name.into(),
                receiver: "model_colon".into(),
                arity_including_receiver: method.arity(),
                effect: method.effect,
                model_eligibility: method.eligibility,
                arguments: method
                    .arguments
                    .iter()
                    .map(|arg| ArgumentDoc {
                        name: arg.name.into(),
                        required: true,
                        value_type: arg.ty,
                    })
                    .collect(),
                returns: method.returns,
                rule_ids: vec![method.rule.id.into()],
                example_ids: vec![method.rule.example.into()],
            },
        )?;
    }
    for (method, options) in [
        ("model.find", api::FIND_OPTIONS),
        ("model.count", api::COUNT_OPTIONS),
    ] {
        for option in options {
            let (value_type, default, maximum) = match option.ty {
                OptionType::Where => (OptionKind::Predicate, None, None),
                OptionType::OrderBy => (OptionKind::SortKeys, None, Some(api::query::SORT_KEYS)),
                OptionType::Integer(value) => (
                    OptionKind::NonnegativeInteger,
                    value.default,
                    Some(value.maximum),
                ),
            };
            budget.push(
                &mut doc.options,
                OptionDoc {
                    id: format!("{method}.{}", option.name),
                    method_id: method.into(),
                    name: option.name.into(),
                    value_type,
                    required: false,
                    nullable: option.nullable,
                    default,
                    maximum,
                },
            )?;
        }
    }
    for operator in api::query::OPERATORS {
        budget.push(
            &mut doc.operators,
            OperatorDoc {
                id: format!("operator.{}", operator.name),
                name: operator.name.into(),
                eligibility: operator.eligibility,
                operand: operator.operand,
                rule_ids: vec![operator.rule.id.into()],
                example_ids: vec![operator.rule.example.into()],
            },
        )?;
    }
    for (name, _) in api::query::DIRECTIONS {
        budget.push(&mut doc.sort_directions, (*name).into())?;
    }
    for rule in api::METHODS
        .iter()
        .map(|m| &m.rule)
        .chain(api::semantics::RULES)
        .chain(api::constraints::RULES)
        .chain(api::query::OPERATORS.iter().map(|operator| &operator.rule))
    {
        if !api::examples::EXAMPLES
            .iter()
            .any(|example| example.id == rule.example)
        {
            return Err(ReferenceError::contract());
        }
        budget.push(
            &mut doc.rules,
            RuleDoc {
                id: rule.id.into(),
                description: rule.description.into(),
                failure_class: rule.failure,
                example_ids: vec![rule.example.into()],
            },
        )?;
    }
    for example in api::examples::EXAMPLES {
        budget.push(
            &mut doc.examples,
            ExampleDoc {
                id: example.id.into(),
                title: example.title.into(),
                origin: ExampleOrigin::GenericIllustration,
                scope: example.effect,
                luau: example.body.into(),
                rule_ids: doc
                    .rules
                    .iter()
                    .filter(|rule| rule.example_ids.iter().any(|id| id == example.id))
                    .map(|rule| rule.id.clone())
                    .collect(),
            },
        )?;
    }
    for (id, effect) in [
        ("query_root", Effect::Read),
        ("mutation_root", Effect::Write),
        ("nested", Effect::Read),
    ] {
        budget.push(
            &mut doc.contexts,
            ContextDoc {
                id: id.into(),
                type_id: "context".into(),
                scope_value: if effect == Effect::Write {
                    api::context::MUTATION
                } else {
                    api::context::READ
                }
                .into(),
                effect,
                method_ids: api::METHODS
                    .iter()
                    .filter(|method| method.effect.allowed(effect == Effect::Write))
                    .map(|method| method.id.into())
                    .collect(),
                rule_ids: vec![
                    "scope.write".into(),
                    "transaction.snapshot".into(),
                    "transaction.rollback".into(),
                ],
            },
        )?;
    }
    for name in crate::api::runtime::paths().chain(std::iter::once("_G".into())) {
        budget.push(&mut doc.globals, name)?;
    }
    Ok(())
}
