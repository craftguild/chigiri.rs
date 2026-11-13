//! Build one explanatory document from the API contract and Mock IR without claiming readiness.

mod api_types;
mod budget;
mod common;
mod constraint_limits;
mod construction;
mod context_graph;
mod directive_metadata;
mod directive_types;
mod directives;
mod event_text;
mod event_types;
mod events;
mod hook_graph;
mod hooks;
mod input_text;
mod limits;
mod man;
mod request_limits;
mod runtime_events;
mod schema;
mod schema_models;
mod schema_types;
mod schema_values;
mod selection;
mod state_graph;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_descriptions;
#[cfg(test)]
mod tests_directives;
#[cfg(test)]
mod tests_events;
#[cfg(test)]
mod tests_hooks;
#[cfg(test)]
mod tests_input_text;
#[cfg(test)]
mod tests_mutations;
#[cfg(test)]
mod tests_runtime;
#[cfg(test)]
mod tests_simplified;
#[cfg(test)]
mod tests_subscription_surface;
#[cfg(test)]
mod tests_topics;
#[cfg(test)]
mod tests_topics_directives;
#[cfg(test)]
mod tests_type_graph;
#[cfg(test)]
mod tests_versions;
mod text;
mod text_directives;
mod text_schema;
mod topic;
mod topic_directives;
pub(crate) mod topic_runtime;
mod topic_schema;
mod topic_schema_types;
mod topic_text;
mod topic_text_detail;
mod topic_text_schema;
mod topic_types;
mod type_graph;
mod versions;

use crate::{info, ir::MockIr};
use api_types::*;
use schema_types::SchemaDoc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Write;

pub use selection::{Selection, Topic};

/// Maximum size in bytes, enforced separately for each rendered reference output.
pub const MAX_REFERENCE_BYTES: usize = 16 * 1024 * 1024;
/// Versioned JSON format used by topic-specific references.
pub const REFERENCE_TOPIC_FORMAT: &str = topic_types::FORMAT;

/// Reference generating error that does not include input text or environment values.
#[derive(Debug)]
pub struct ReferenceError(&'static str);

impl ReferenceError {
    /// Return a fixed classification without input values for CLI diagnostics.
    pub fn message(&self) -> &'static str {
        self.0
    }
    fn limit() -> Self {
        Self("reference exceeds 16 MiB")
    }
    fn contract() -> Self {
        Self("invalid reference contract")
    }
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.0)
    }
}
impl std::error::Error for ReferenceError {}

/// Reference rendering format.
#[derive(Clone, Copy)]
pub enum Format {
    /// Plain UTF-8 for people.
    Text,
    /// Closed JSON reference format.
    Json,
}

/// Reference input boundary. Only project input evaluates Luau configuration.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    /// Common API without schema input.
    Common,
    /// Explicit SDL input.
    Schema,
    /// Project input that reads adopted public SDL and current project-local SDL.
    Project,
    /// Portable bundle input with a validated closed envelope and SDL.
    Bundle,
}

impl InputKind {
    fn name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Schema => "schema",
            Self::Project => "project",
            Self::Bundle => "bundle",
        }
    }
}

/// Serializable reference document derived from public contracts and optional schema metadata.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceDocument {
    #[serde(deserialize_with = "versions::current_format")]
    format: String,
    #[serde(deserialize_with = "versions::current_api")]
    api_version: String,
    #[serde(deserialize_with = "versions::current_mock")]
    mock_format: Option<String>,
    supported_formats: SupportedFormats,
    input_kind: InputKind,
    config_executed: bool,
    binary: BinaryDoc,
    readiness_checked: bool,
    directives: Vec<directive_types::DirectiveDoc>,
    methods: Vec<MethodDoc>,
    api_types: Vec<type_graph::ApiTypeDoc>,
    options: Vec<OptionDoc>,
    operators: Vec<OperatorDoc>,
    sort_directions: Vec<String>,
    rules: Vec<RuleDoc>,
    examples: Vec<ExampleDoc>,
    limits: Vec<LimitDoc>,
    contexts: Vec<ContextDoc>,
    hooks: Vec<hooks::HookDoc>,
    state_configured: bool,
    rejection: hooks::RejectionDoc,
    logging: hooks::LoggingDoc,
    events: event_types::EventsDoc,
    globals: Vec<String>,
    schema: Option<SchemaDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BinaryDoc {
    version: String,
    luaur: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportedFormats {
    #[serde(deserialize_with = "versions::current_project")]
    project: String,
    #[serde(deserialize_with = "versions::current_bundle")]
    bundle: String,
}

impl ReferenceDocument {
    /// Render a complete document within 16 MiB without exposing partial output.
    pub fn render(&self, format: Format) -> Result<Vec<u8>, ReferenceError> {
        let mut out = budget::Buffer::new();
        match format {
            Format::Json => {
                serde_json::to_writer(&mut out, self).map_err(|_| ReferenceError::limit())?;
                out.write_all(b"\n").map_err(|_| ReferenceError::limit())?;
            }
            Format::Text => text::render(self, &mut out).map_err(|_| ReferenceError::limit())?,
        }
        Ok(out.finish())
    }

    /// Check that a topic/name selection exists and is a permitted combination.
    pub fn accepts(&self, selection: &Selection) -> bool {
        let Some(topic) = selection.topic else {
            return selection.name.is_none();
        };
        if selection.name.is_some() && !topic.accepts_name() {
            return false;
        }
        if topic == Topic::All {
            return selection.name.is_none();
        }
        let Some(name) = selection.name.as_deref() else {
            return true;
        };
        match topic {
            Topic::Directives => self.directives.iter().any(|item| item.name == name),
            Topic::Methods => self.methods.iter().any(|item| item.name == name),
            Topic::Hooks => self.hooks.iter().any(|item| item.slot.name() == name),
            Topic::Models => self.schema.as_ref().is_some_and(|schema| {
                schema.models.iter().any(|model| {
                    schema
                        .types
                        .iter()
                        .any(|ty| ty.id == model.type_id && ty.name == name)
                })
            }),
            Topic::Types => self.schema.as_ref().is_some_and(|schema| {
                schema.types.iter().any(|ty| {
                    ty.name == name && ty.visibility == schema_types::VisibilityDoc::Public
                })
            }),
            Topic::Resolvers => self.schema.as_ref().is_some_and(|schema| {
                schema
                    .resolvers
                    .iter()
                    .any(|resolver| resolver.coordinate == name)
            }),
            Topic::Consumers => self.events.consumers.iter().any(|item| item.name == name),
            Topic::Runtime => topic_runtime::build(self)
                .iter()
                .any(|item| item.name == name),
            Topic::Limits | Topic::All => false,
        }
    }

    /// Render landing text, a complete document, or closed topic JSON from one document.
    pub fn render_selection(
        &self,
        format: Format,
        selection: &Selection,
    ) -> Result<Vec<u8>, ReferenceError> {
        if !self.accepts(selection) {
            return Err(ReferenceError::contract());
        }
        if matches!(format, Format::Json) && matches!(selection.topic, None | Some(Topic::All))
            || matches!(format, Format::Text) && selection.topic == Some(Topic::All)
        {
            return self.render(format);
        }
        let mut out = budget::Buffer::new();
        match format {
            Format::Text if selection.topic.is_none() => {
                topic_text::landing(self, &mut out).map_err(|_| ReferenceError::limit())?;
            }
            Format::Text => {
                let topic = topic::build(self, selection)?.ok_or_else(ReferenceError::contract)?;
                topic_text::render(&topic, &mut out).map_err(|_| ReferenceError::limit())?;
            }
            Format::Json => {
                let topic = topic::build(self, selection)?.ok_or_else(ReferenceError::contract)?;
                serde_json::to_writer(&mut out, &topic).map_err(|_| ReferenceError::limit())?;
                out.write_all(b"\n").map_err(|_| ReferenceError::limit())?;
            }
        }
        Ok(out.finish())
    }
}
