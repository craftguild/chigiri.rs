//! Closed source and consumer discovery values, independent of runtime activation.

use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

use super::schema_types::ConstraintsDoc;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventsDoc {
    pub configured: bool,
    pub limits: crate::events::SourceLimits,
    pub types: Vec<EventTypeDoc>,
    pub consumers: Vec<ConsumerDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventTypeDoc {
    pub name: String,
    pub publish_signature: String,
    pub publish_input_definition: String,
    pub destinations: Vec<EventDestinationDoc>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventDestinationDoc {
    pub coordinate: String,
    pub description: Option<String>,
    pub payload_type: String,
    pub source_type: String,
    pub pubsub_signature: String,
    pub consumer_pubsub_signature: String,
    pub generator_signature: String,
    pub sequence_input_definition: String,
    pub graphql_subscribe_signature: String,
    pub consumer_subscribe_signature: String,
    pub consume_signature: String,
    pub constraints: ConstraintsDoc,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConsumerDoc {
    pub name: String,
    pub enabled: bool,
    pub module_path: PortableRelativePath,
    pub source: EventDestinationDoc,
    pub setup_authority: String,
    pub execution_authority: String,
}
