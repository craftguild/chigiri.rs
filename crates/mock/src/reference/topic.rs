//! Select a self-contained topic from the complete reference document while preserving graph links.

use super::{
    ReferenceDocument, ReferenceError,
    api_types::{LimitUnit, OptionKind},
    selection::{Selection, Topic},
    topic_types::*,
};
pub(super) fn build(
    doc: &ReferenceDocument,
    selection: &Selection,
) -> Result<Option<TopicDocument>, ReferenceError> {
    let Some(topic) = selection.topic else {
        return Ok(None);
    };
    let content = match topic {
        Topic::Directives => TopicContent::Directives(filter(
            super::topic_directives::build(doc)?,
            selection.name.as_deref(),
            |item| &item.name,
        )),
        Topic::Methods => {
            TopicContent::Methods(filter(methods(doc)?, selection.name.as_deref(), |item| {
                &item.name
            }))
        }
        Topic::Hooks => {
            TopicContent::Hooks(filter(hooks(doc)?, selection.name.as_deref(), |item| {
                &item.name
            }))
        }
        Topic::Models => {
            TopicContent::Models(filter(models(doc)?, selection.name.as_deref(), |item| {
                &item.name
            }))
        }
        Topic::Types => TopicContent::Types(filter(
            super::topic_schema::types(doc)?,
            selection.name.as_deref(),
            |item| &item.name,
        )),
        Topic::Resolvers => TopicContent::Resolvers(filter(
            super::topic_schema::resolvers(doc)?,
            selection.name.as_deref(),
            |item| &item.coordinate,
        )),
        Topic::Consumers => TopicContent::Consumers(filter(
            doc.events.consumers.clone(),
            selection.name.as_deref(),
            |item| &item.name,
        )),
        Topic::Limits => TopicContent::Limits(limits(doc)),
        Topic::Runtime => TopicContent::Runtime(filter(
            super::topic_runtime::build(doc),
            selection.name.as_deref(),
            |item| &item.name,
        )),
        Topic::All => return Ok(None),
    };
    if selection.name.is_some() && content.len() != 1 {
        return Ok(None);
    }
    Ok(Some(TopicDocument {
        format: FORMAT.into(),
        reference_format: crate::info::REFERENCE_FORMAT.into(),
        api_version: crate::info::API_VERSION.into(),
        input_kind: doc.input_kind,
        readiness_checked: false,
        selection: TopicSelection {
            topic,
            name: selection.name.clone(),
        },
        content,
    }))
}

fn filter<T>(values: Vec<T>, name: Option<&str>, get: impl Fn(&T) -> &str) -> Vec<T> {
    match name {
        None => values,
        Some(name) => values
            .into_iter()
            .filter(|value| get(value) == name)
            .collect(),
    }
}

fn methods(doc: &ReferenceDocument) -> Result<Vec<MethodTopicDoc>, ReferenceError> {
    doc.methods
        .iter()
        .map(|method| {
            let rule = doc
                .rules
                .iter()
                .find(|rule| method.rule_ids.contains(&rule.id))
                .ok_or_else(ReferenceError::contract)?;
            let mut signature = Vec::new();
            super::text::signature(&mut signature, method, "M")
                .map_err(|_| ReferenceError::contract())?;
            Ok(MethodTopicDoc {
                id: method.id.clone(),
                name: method.name.clone(),
                purpose: rule.description.clone(),
                effect: super::text::effect(method.effect).into(),
                eligibility: serde_name(&method.model_eligibility)?,
                signature: String::from_utf8(signature)
                    .map_err(|_| ReferenceError::contract())?
                    .trim_end()
                    .into(),
                options: doc
                    .options
                    .iter()
                    .filter(|option| option.method_id == method.id)
                    .map(|option| MethodOptionTopicDoc {
                        name: option.name.clone(),
                        value_type: option_kind(option.value_type).into(),
                        required: option.required,
                        nullable: option.nullable,
                        default: option.default,
                        maximum: option.maximum,
                    })
                    .collect(),
                rules: doc
                    .rules
                    .iter()
                    .filter(|row| method.rule_ids.contains(&row.id))
                    .map(|row| row.description.clone())
                    .collect(),
                examples: doc
                    .examples
                    .iter()
                    .filter(|example| method.example_ids.contains(&example.id))
                    .map(|example| CodeExampleTopicDoc {
                        title: example.title.clone(),
                        code: example.luau.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

fn hooks(doc: &ReferenceDocument) -> Result<Vec<HookTopicDoc>, ReferenceError> {
    doc.hooks
        .iter()
        .map(|hook| {
            let example = doc
                .examples
                .iter()
                .find(|example| example.id == hook.example_id)
                .ok_or_else(ReferenceError::contract)?;
            Ok(HookTopicDoc {
                name: hook.slot.name().into(),
                purpose: format!(
                    "{} phase {} hook; models are read-only.",
                    serde_name(&hook.phase)?,
                    serde_name(&hook.timing)?
                ),
                module_path: hook.module_path.clone(),
                enabled: hook.enabled,
                phase: serde_name(&hook.phase)?,
                timing: serde_name(&hook.timing)?,
                state_patch: hook.state_patch,
                reject: hook.reject,
                models_read_only: hook.models_read_only,
                logging_format: doc.logging.format.clone(),
                diagnostic_fields: doc.logging.diagnostic_fields.clone(),
                source_positions_nullable: doc.logging.source_positions_nullable,
                example: CodeExampleTopicDoc {
                    title: example.title.clone(),
                    code: example.luau.clone(),
                },
            })
        })
        .collect()
}

fn models(doc: &ReferenceDocument) -> Result<Vec<ModelTopicDoc>, ReferenceError> {
    let Some(schema) = &doc.schema else {
        return Ok(Vec::new());
    };
    schema
        .models
        .iter()
        .map(|model| {
            let ty = schema
                .types
                .iter()
                .find(|ty| ty.id == model.type_id)
                .ok_or_else(ReferenceError::contract)?;
            let key = model
                .key_field_id
                .as_ref()
                .map(|id| field_name(schema, id))
                .transpose()?;
            let slots = model
                .slot_ids
                .iter()
                .map(|id| {
                    let slot = schema
                        .slots
                        .iter()
                        .find(|slot| &slot.id == id)
                        .ok_or_else(ReferenceError::contract)?;
                    Ok(ModelSlotTopicDoc {
                        name: slot.name.clone(),
                        description: slot.description.as_ref().map(|value| value.text.clone()),
                        value_type: type_reference(schema, &slot.value_type)?,
                        create_required: slot.create_required,
                        writable: slot.writable,
                        operators: slot
                            .filter_operator_ids
                            .iter()
                            .map(|id| id.trim_start_matches("operator.").into())
                            .collect(),
                        sortable: slot.sortable,
                        visibility: serde_name(&slot.visibility)?,
                        source: slot.source.clone(),
                    })
                })
                .collect::<Result<_, ReferenceError>>()?;
            Ok(ModelTopicDoc {
                id: model.id.clone(),
                name: ty.name.clone(),
                description: model.description.as_ref().map(|value| value.text.clone()),
                key,
                methods: model
                    .method_ids
                    .iter()
                    .map(|id| method_name(doc, id))
                    .collect::<Result<_, _>>()?,
                slots,
                relations: relations(schema, model)?,
                visibility: serde_name(&model.visibility)?,
                source: model.source.clone(),
            })
        })
        .collect()
}

fn relations(
    schema: &super::schema_types::SchemaDoc,
    model: &super::schema_types::ModelDoc,
) -> Result<Vec<ModelRelationTopicDoc>, ReferenceError> {
    let mut result = Vec::new();
    for relation in &schema.relations {
        if relation.owner_model_id == model.id {
            let target = model_name(schema, &relation.target_model_id)?;
            result.push(ModelRelationTopicDoc {
                field: field_name(schema, &relation.owner_field_id)?,
                target,
                many: relation.many,
                nullable: relation.nullable,
            });
        }
        if relation.target_model_id == model.id {
            let target = model_name(schema, &relation.owner_model_id)?;
            for field_id in &relation.inverse_field_ids {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| &field.id == field_id && field.parent_type_id == model.type_id)
                    .ok_or_else(ReferenceError::contract)?;
                result.push(ModelRelationTopicDoc {
                    field: field.name.clone(),
                    target: target.clone(),
                    many: field
                        .value_type
                        .wrappers
                        .iter()
                        .any(|wrapper| matches!(wrapper, super::schema_types::WrapperDoc::List)),
                    nullable: !matches!(
                        field.value_type.wrappers.first(),
                        Some(super::schema_types::WrapperDoc::NonNull)
                    ),
                });
            }
        }
    }
    Ok(result)
}

fn limits(doc: &ReferenceDocument) -> Vec<LimitTopicDoc> {
    doc.limits
        .iter()
        .map(|limit| LimitTopicDoc {
            id: limit.id.clone(),
            value: limit.value,
            unit: limit_unit(limit.unit).into(),
            scope: limit.shared_scope.clone(),
            purpose: limit.description.clone(),
        })
        .collect()
}

pub(super) fn method_name(doc: &ReferenceDocument, id: &str) -> Result<String, ReferenceError> {
    doc.methods
        .iter()
        .find(|method| method.id == id)
        .map(|method| method.name.clone())
        .ok_or_else(ReferenceError::contract)
}

pub(super) fn model_name(
    schema: &super::schema_types::SchemaDoc,
    id: &str,
) -> Result<String, ReferenceError> {
    let model = schema
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(ReferenceError::contract)?;
    schema
        .types
        .iter()
        .find(|ty| ty.id == model.type_id)
        .map(|ty| ty.name.clone())
        .ok_or_else(ReferenceError::contract)
}

fn field_name(schema: &super::schema_types::SchemaDoc, id: &str) -> Result<String, ReferenceError> {
    schema
        .fields
        .iter()
        .find(|field| field.id == id)
        .map(|field| field.name.clone())
        .ok_or_else(ReferenceError::contract)
}

pub(super) fn type_reference(
    schema: &super::schema_types::SchemaDoc,
    reference: &super::schema_types::TypeRefDoc,
) -> Result<String, ReferenceError> {
    let ty = schema
        .types
        .iter()
        .find(|ty| ty.id == reference.type_id)
        .ok_or_else(ReferenceError::contract)?;
    let mut value = String::new();
    for wrapper in &reference.wrappers {
        if matches!(wrapper, super::schema_types::WrapperDoc::List) {
            value.push('[');
        }
    }
    value.push_str(&ty.name);
    for wrapper in reference.wrappers.iter().rev() {
        value.push(match wrapper {
            super::schema_types::WrapperDoc::NonNull => '!',
            super::schema_types::WrapperDoc::List => ']',
        });
    }
    Ok(value)
}

pub(super) fn serde_name(value: &impl serde::Serialize) -> Result<String, ReferenceError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(ReferenceError::contract)
}

fn option_kind(value: OptionKind) -> &'static str {
    match value {
        OptionKind::Predicate => "predicate",
        OptionKind::SortKeys => "sort_keys",
        OptionKind::NonnegativeInteger => "nonnegative_integer",
    }
}

fn limit_unit(value: LimitUnit) -> &'static str {
    match value {
        LimitUnit::Bytes => "bytes",
        LimitUnit::Nodes => "nodes",
        LimitUnit::Items => "items",
        LimitUnit::Calls => "calls",
        LimitUnit::Work => "work",
        LimitUnit::Milliseconds => "milliseconds",
        LimitUnit::Depth => "depth",
    }
}
