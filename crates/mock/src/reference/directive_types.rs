//! Closed wire types for the directive catalog.

use super::schema_types::{DescriptionDoc, LiteralDoc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectiveDoc {
    pub id: String,
    pub name: String,
    pub class: DirectiveClass,
    pub execution: DirectiveExecution,
    pub purpose: String,
    pub declared_description: Option<DescriptionDoc>,
    pub repeatable: bool,
    pub locations: Vec<String>,
    pub syntax: String,
    pub arguments: Vec<DirectiveArgumentDoc>,
    pub rule_ids: Vec<String>,
    pub examples: Vec<DirectiveExampleDoc>,
    pub related: Vec<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DirectiveClass {
    GraphqlStandard,
    ChigiriMock,
    UserDeclared,
}

impl DirectiveClass {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::GraphqlStandard => "GraphQL standard",
            Self::ChigiriMock => "Chigiri Mock",
            Self::UserDeclared => "User SDL declaration",
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DirectiveExecution {
    Graphql,
    ChigiriMock,
    DeclarationOnly,
}

impl DirectiveExecution {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Graphql => "GraphQL execution",
            Self::ChigiriMock => "Mock validation and execution",
            Self::DeclarationOnly => "Declaration only",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectiveArgumentDoc {
    pub name: String,
    pub value_type: String,
    pub required: bool,
    pub default: Option<LiteralDoc>,
    pub choices: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectiveExampleDoc {
    pub id: String,
    pub title: String,
    pub valid: bool,
    pub sdl: String,
    pub expected: String,
}

/// Topic representation with related rule details embedded in one item.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectiveTopicDoc {
    pub id: String,
    pub name: String,
    pub class: DirectiveClass,
    pub execution: DirectiveExecution,
    pub purpose: String,
    pub declared_description: Option<DescriptionDoc>,
    pub repeatable: bool,
    pub locations: Vec<String>,
    pub syntax: String,
    pub arguments: Vec<DirectiveArgumentDoc>,
    pub rule_ids: Vec<String>,
    pub rules: Vec<DirectiveRuleTopicDoc>,
    pub examples: Vec<DirectiveExampleDoc>,
    pub related: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectiveRuleTopicDoc {
    pub id: String,
    pub description: String,
    pub failure_class: String,
}

impl DirectiveTopicDoc {
    pub(super) fn catalog_document(&self) -> DirectiveDoc {
        DirectiveDoc {
            id: self.id.clone(),
            name: self.name.clone(),
            class: self.class,
            execution: self.execution,
            purpose: self.purpose.clone(),
            declared_description: self.declared_description.clone(),
            repeatable: self.repeatable,
            locations: self.locations.clone(),
            syntax: self.syntax.clone(),
            arguments: self.arguments.clone(),
            rule_ids: self.rule_ids.clone(),
            examples: self.examples.clone(),
            related: self.related.clone(),
        }
    }
}
