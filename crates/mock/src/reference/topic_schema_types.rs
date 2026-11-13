//! Closed schema-derived values used by scoped type and resolver topics.

use super::schema_types::{ConstraintsDoc, LiteralDoc, ScalarDoc};
use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeTopicDoc {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub model: bool,
    pub fields: Vec<TypeFieldTopicDoc>,
    pub inputs: Vec<InputTopicDoc>,
    pub enum_values: Vec<EnumValueTopicDoc>,
    pub interfaces: Vec<String>,
    pub possible_types: Vec<String>,
    pub one_of: bool,
    pub scalar: Option<ScalarDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeFieldTopicDoc {
    pub name: String,
    pub description: Option<String>,
    pub value_type: String,
    pub arguments: Vec<InputTopicDoc>,
    pub constraints: ConstraintsDoc,
    pub role: TypeFieldRoleTopicDoc,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum TypeFieldRoleTopicDoc {
    Stored {
        model: String,
        slot: String,
        model_key: bool,
    },
    Value,
    Owner {
        relation: FieldRelationTopicDoc,
    },
    Inverse {
        relation: FieldRelationTopicDoc,
    },
    Resolver {
        coordinate: String,
        module_path: PortableRelativePath,
    },
    Declaration,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldRelationTopicDoc {
    pub name: Option<String>,
    pub owner_model: String,
    pub owner_field: String,
    pub target_model: String,
    pub storage_keys: Vec<String>,
    pub reference_keys: Vec<String>,
    pub inverse_fields: Vec<String>,
    pub implicit: bool,
    pub many: bool,
    pub nullable: bool,
    pub nullable_items: Option<bool>,
    pub unique: bool,
    pub min_refs: usize,
    pub max_refs: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InputTopicDoc {
    pub name: String,
    pub description: Option<String>,
    pub value_type: String,
    pub default: Option<LiteralDoc>,
    pub required_after_coercion: bool,
    pub constraints: ConstraintsDoc,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnumValueTopicDoc {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolverTopicDoc {
    pub id: String,
    pub coordinate: String,
    pub description: Option<String>,
    pub module_path: PortableRelativePath,
    pub arguments: Vec<InputTopicDoc>,
    pub returns: String,
    pub scopes: Vec<ResolverScopeTopicDoc>,
    pub subscription_source: Option<super::event_types::EventDestinationDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolverScopeTopicDoc {
    pub scope: String,
    pub models: Vec<ModelMethodsTopicDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelMethodsTopicDoc {
    pub model: String,
    pub methods: Vec<String>,
}
