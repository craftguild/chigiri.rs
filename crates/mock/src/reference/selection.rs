//! Closed topic selection shared by text and JSON output.

use serde::{Deserialize, Serialize};

/// User-facing reference topic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Mock directive ownership and user declarations.
    Directives,
    /// Seven model operations.
    Methods,
    /// Six fixed hook slots and their capabilities.
    Hooks,
    /// Storage models derived from the schema.
    Models,
    /// GraphQL types derived from the selected schema.
    Types,
    /// Resolver contracts derived from the selected schema.
    Resolvers,
    /// Explicit named transactional consumers, including disabled registrations.
    Consumers,
    /// Resource limits shared with execution.
    Limits,
    /// Executable runtime surfaces and their phase restrictions.
    Runtime,
    /// Complete reference with IDs and type graph.
    All,
}

impl Topic {
    /// Parse a closed CLI value into its internal topic.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "directives" => Self::Directives,
            "methods" => Self::Methods,
            "hooks" => Self::Hooks,
            "models" => Self::Models,
            "types" => Self::Types,
            "resolvers" => Self::Resolvers,
            "limits" => Self::Limits,
            "runtime" => Self::Runtime,
            "consumers" => Self::Consumers,
            "all" => Self::All,
            _ => return None,
        })
    }

    /// CLI and JSON share a common name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Directives => "directives",
            Self::Methods => "methods",
            Self::Hooks => "hooks",
            Self::Models => "models",
            Self::Types => "types",
            Self::Resolvers => "resolvers",
            Self::Limits => "limits",
            Self::Runtime => "runtime",
            Self::Consumers => "consumers",
            Self::All => "all",
        }
    }

    pub(super) const fn accepts_name(self) -> bool {
        matches!(
            self,
            Self::Directives
                | Self::Methods
                | Self::Hooks
                | Self::Models
                | Self::Types
                | Self::Resolvers
                | Self::Runtime
                | Self::Consumers
        )
    }
}

/// Optional topic and exact item name selected from CLI arguments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Omitted means landing text for text output and the complete document for JSON.
    pub topic: Option<Topic>,
    /// Exact name in topic.
    pub name: Option<String>,
}

impl Selection {
    /// Validate syntax only; the completed document validates an exact name.
    pub fn new(topic: Option<Topic>, name: Option<String>) -> Option<Self> {
        if name.is_some() && !topic.is_some_and(Topic::accepts_name) {
            return None;
        }
        Some(Self { topic, name })
    }
}
