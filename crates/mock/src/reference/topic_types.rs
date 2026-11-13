//! Self-contained JSON types for topic documents.

use super::{directive_types::DirectiveTopicDoc, selection::Topic};
use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

pub(super) use super::topic_schema_types::*;

pub(super) const FORMAT: &str = "chigiri-mock-reference-topic/v9";

#[derive(Serialize)]
pub(super) struct TopicDocument {
    pub format: String,
    pub reference_format: String,
    pub api_version: String,
    pub input_kind: super::InputKind,
    pub readiness_checked: bool,
    pub selection: TopicSelection,
    pub content: TopicContent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TopicDocumentWire {
    #[serde(deserialize_with = "current_topic")]
    format: String,
    #[serde(deserialize_with = "current_reference")]
    reference_format: String,
    #[serde(deserialize_with = "current_api")]
    api_version: String,
    input_kind: super::InputKind,
    readiness_checked: bool,
    selection: TopicSelection,
    content: TopicContent,
}

impl<'de> Deserialize<'de> for TopicDocument {
    fn deserialize<D: serde::Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        let wire = TopicDocumentWire::deserialize(reader)?;
        if wire.readiness_checked
            || wire.selection.topic != wire.content.topic()
            || wire.selection.name.is_some() && !wire.selection.topic.accepts_name()
            || !wire.content.matches_name(wire.selection.name.as_deref())
        {
            return Err(serde::de::Error::custom(
                "inconsistent reference topic selection",
            ));
        }
        Ok(Self {
            format: wire.format,
            reference_format: wire.reference_format,
            api_version: wire.api_version,
            input_kind: wire.input_kind,
            readiness_checked: wire.readiness_checked,
            selection: wire.selection,
            content: wire.content,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TopicSelection {
    pub topic: Topic,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "items",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum TopicContent {
    Directives(Vec<DirectiveTopicDoc>),
    Methods(Vec<MethodTopicDoc>),
    Hooks(Vec<HookTopicDoc>),
    Models(Vec<ModelTopicDoc>),
    Types(Vec<TypeTopicDoc>),
    Resolvers(Vec<ResolverTopicDoc>),
    Limits(Vec<LimitTopicDoc>),
    Runtime(Vec<RuntimeTopicDoc>),
    Consumers(Vec<super::event_types::ConsumerDoc>),
}

impl TopicContent {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Directives(items) => items.len(),
            Self::Methods(items) => items.len(),
            Self::Hooks(items) => items.len(),
            Self::Models(items) => items.len(),
            Self::Types(items) => items.len(),
            Self::Resolvers(items) => items.len(),
            Self::Limits(items) => items.len(),
            Self::Runtime(items) => items.len(),
            Self::Consumers(items) => items.len(),
        }
    }

    fn topic(&self) -> Topic {
        match self {
            Self::Directives(_) => Topic::Directives,
            Self::Methods(_) => Topic::Methods,
            Self::Hooks(_) => Topic::Hooks,
            Self::Models(_) => Topic::Models,
            Self::Types(_) => Topic::Types,
            Self::Resolvers(_) => Topic::Resolvers,
            Self::Limits(_) => Topic::Limits,
            Self::Runtime(_) => Topic::Runtime,
            Self::Consumers(_) => Topic::Consumers,
        }
    }

    fn matches_name(&self, selected: Option<&str>) -> bool {
        let Some(selected) = selected else {
            return true;
        };
        if self.len() != 1 {
            return false;
        }
        match self {
            Self::Directives(items) => items[0].name == selected,
            Self::Methods(items) => items[0].name == selected,
            Self::Models(items) => items[0].name == selected,
            Self::Types(items) => items[0].name == selected,
            Self::Resolvers(items) => items[0].coordinate == selected,
            Self::Hooks(items) => items[0].name == selected,
            Self::Runtime(items) => items[0].name == selected,
            Self::Consumers(items) => items[0].name == selected,
            Self::Limits(_) => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MethodTopicDoc {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub effect: String,
    pub eligibility: String,
    pub signature: String,
    pub options: Vec<MethodOptionTopicDoc>,
    pub rules: Vec<String>,
    pub examples: Vec<CodeExampleTopicDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MethodOptionTopicDoc {
    pub name: String,
    pub value_type: String,
    pub required: bool,
    pub nullable: bool,
    pub default: Option<usize>,
    pub maximum: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeExampleTopicDoc {
    pub title: String,
    pub code: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HookTopicDoc {
    pub name: String,
    pub purpose: String,
    pub module_path: PortableRelativePath,
    pub enabled: Option<bool>,
    pub phase: String,
    pub timing: String,
    pub state_patch: bool,
    pub reject: bool,
    pub models_read_only: bool,
    #[serde(deserialize_with = "current_log")]
    pub logging_format: String,
    pub diagnostic_fields: Vec<String>,
    pub source_positions_nullable: bool,
    pub example: CodeExampleTopicDoc,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelTopicDoc {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub key: Option<String>,
    pub methods: Vec<String>,
    pub slots: Vec<ModelSlotTopicDoc>,
    pub relations: Vec<ModelRelationTopicDoc>,
    pub visibility: String,
    pub source: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelSlotTopicDoc {
    pub name: String,
    pub description: Option<String>,
    pub value_type: String,
    pub create_required: bool,
    pub writable: bool,
    pub operators: Vec<String>,
    pub sortable: bool,
    pub visibility: String,
    pub source: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelRelationTopicDoc {
    pub field: String,
    pub target: String,
    pub many: bool,
    pub nullable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LimitTopicDoc {
    pub id: String,
    pub value: usize,
    pub unit: String,
    pub scope: String,
    pub purpose: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeTopicDoc {
    pub name: String,
    pub purpose: String,
    pub availability: Vec<String>,
    pub signatures: Vec<String>,
    pub limits: Vec<String>,
    pub examples: Vec<CodeExampleTopicDoc>,
}

fn current_topic<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    exact(reader, FORMAT, "unsupported reference topic format")
}
fn current_reference<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    exact(
        reader,
        crate::info::REFERENCE_FORMAT,
        "unsupported reference format",
    )
}
fn current_api<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    exact(reader, crate::info::API_VERSION, "unsupported host API")
}
fn current_log<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    exact(
        reader,
        crate::logging::FORMAT,
        "unsupported runtime log format",
    )
}
fn exact<'de, D: serde::Deserializer<'de>>(
    reader: D,
    expected: &str,
    message: &str,
) -> Result<String, D::Error> {
    let value = String::deserialize(reader)?;
    if value != expected {
        return Err(serde::de::Error::custom(message));
    }
    Ok(value)
}
