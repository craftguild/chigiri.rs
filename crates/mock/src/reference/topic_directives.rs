//! Build a self-contained directive topic without depending on full-document IDs.

use super::{
    ReferenceDocument, ReferenceError,
    directive_types::{DirectiveRuleTopicDoc, DirectiveTopicDoc},
};

pub(super) fn build(doc: &ReferenceDocument) -> Result<Vec<DirectiveTopicDoc>, ReferenceError> {
    doc.directives
        .iter()
        .map(|item| {
            let rules = item
                .rule_ids
                .iter()
                .map(|id| {
                    let rule = doc
                        .rules
                        .iter()
                        .find(|rule| &rule.id == id)
                        .ok_or_else(ReferenceError::contract)?;
                    Ok(DirectiveRuleTopicDoc {
                        id: rule.id.clone(),
                        description: rule.description.clone(),
                        failure_class: serde_name(&rule.failure_class)?,
                    })
                })
                .collect::<Result<_, ReferenceError>>()?;
            Ok(DirectiveTopicDoc {
                id: item.id.clone(),
                name: item.name.clone(),
                class: item.class,
                execution: item.execution,
                purpose: item.purpose.clone(),
                declared_description: item.declared_description.clone(),
                repeatable: item.repeatable,
                locations: item.locations.clone(),
                syntax: item.syntax.clone(),
                arguments: item.arguments.clone(),
                rule_ids: item.rule_ids.clone(),
                rules,
                examples: item.examples.clone(),
                related: item.related.clone(),
            })
        })
        .collect()
}

fn serde_name(value: &impl serde::Serialize) -> Result<String, ReferenceError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(ReferenceError::contract)
}
