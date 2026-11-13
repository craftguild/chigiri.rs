//! Flat ID-referenced schema graph that avoids recursive or shared DTO ownership.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SchemaDoc {
    pub digest: String,
    pub vocabulary: String,
    pub description: Option<DescriptionDoc>,
    pub query_type_id: String,
    pub mutation_type_id: Option<String>,
    pub subscription_type_id: Option<String>,
    pub types: Vec<TypeDoc>,
    pub fields: Vec<FieldDoc>,
    pub inputs: Vec<InputDoc>,
    pub models: Vec<ModelDoc>,
    pub slots: Vec<SlotDoc>,
    pub relations: Vec<RelationDoc>,
    pub resolvers: Vec<ResolverDoc>,
    pub capabilities: Vec<CapabilityDoc>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DescriptionDoc {
    pub origin: DescriptionOrigin,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DescriptionOrigin {
    UserSdl,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeDoc {
    pub id: String,
    pub name: String,
    pub kind: TypeKindDoc,
    pub description: Option<DescriptionDoc>,
    pub model_id: Option<String>,
    pub field_ids: Vec<String>,
    pub input_ids: Vec<String>,
    pub enum_values: Vec<EnumValueDoc>,
    pub interface_ids: Vec<String>,
    pub possible_type_ids: Vec<String>,
    pub one_of: bool,
    pub scalar: Option<ScalarDoc>,
    pub visibility: VisibilityDoc,
}

#[derive(Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum VisibilityDoc {
    Public,
    Storage,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TypeKindDoc {
    Scalar,
    Enum,
    Object,
    Interface,
    Union,
    InputObject,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnumValueDoc {
    pub name: String,
    pub description: Option<DescriptionDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeRefDoc {
    pub type_id: String,
    pub wrappers: Vec<WrapperDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WrapperDoc {
    NonNull,
    List,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScalarDoc {
    pub representation: RepresentationDoc,
    pub constraints: ConstraintsDoc,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RepresentationDoc {
    String,
    Boolean,
    Integer,
    Number,
    Object,
    Array,
    Json,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConstraintsDoc {
    pub exclusive_min: Option<f64>,
    pub exclusive_max: Option<f64>,
    pub multiple_of: Option<f64>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub starts_with: Option<String>,
    pub ends_with: Option<String>,
    pub contains: Option<String>,
    pub not_contains: Option<String>,
    pub pattern: Option<String>,
    pub format: Option<crate::ir::Format>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub min_bytes: Option<usize>,
    pub max_bytes: Option<usize>,
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FieldDoc {
    pub id: String,
    pub parent_type_id: String,
    pub name: String,
    pub description: Option<DescriptionDoc>,
    pub value_type: TypeRefDoc,
    pub argument_ids: Vec<String>,
    pub constraints: ConstraintsDoc,
    pub role: FieldRoleDoc,
    pub visibility: VisibilityDoc,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum FieldRoleDoc {
    Stored { slot_id: String },
    Value,
    Owner { relation_id: String },
    Inverse { relation_id: String },
    Resolver { resolver_id: String },
    Declaration,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InputDoc {
    pub id: String,
    pub name: String,
    pub description: Option<DescriptionDoc>,
    pub value_type: TypeRefDoc,
    pub default: Option<LiteralDoc>,
    pub required_after_coercion: bool,
    pub constraints: ConstraintsDoc,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum LiteralDoc {
    Null,
    Boolean(bool),
    Integer(String),
    Float(String),
    String(String),
    Enum(String),
    List(Vec<LiteralDoc>),
    Object(Vec<LiteralFieldDoc>),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LiteralFieldDoc {
    pub name: String,
    pub value: LiteralDoc,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelDoc {
    pub id: String,
    pub type_id: String,
    pub description: Option<DescriptionDoc>,
    pub key_field_id: Option<String>,
    pub slot_ids: Vec<String>,
    pub relation_ids: Vec<String>,
    pub method_ids: Vec<String>,
    pub api_types: ModelApiTypesDoc,
    pub visibility: VisibilityDoc,
    pub source: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelApiTypesDoc {
    pub record_slot_ids: Vec<String>,
    pub create_slot_ids: Vec<String>,
    pub update_slot_ids: Vec<String>,
    pub predicate_slot_ids: Vec<String>,
    pub sort_slot_ids: Vec<String>,
    pub relation_field_ids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SlotDoc {
    pub id: String,
    pub model_id: String,
    pub field_id: String,
    pub name: String,
    pub description: Option<DescriptionDoc>,
    pub value_type: TypeRefDoc,
    pub constraints: ConstraintsDoc,
    pub model_key: bool,
    pub create_required: bool,
    pub writable: bool,
    pub filter_operator_ids: Vec<String>,
    pub sortable: bool,
    pub visibility: VisibilityDoc,
    pub source: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RelationDoc {
    pub id: String,
    pub owner_model_id: String,
    pub owner_field_id: String,
    pub storage_slot_ids: Vec<String>,
    pub reference_slot_ids: Vec<String>,
    pub implicit: bool,
    pub name: Option<String>,
    pub target_model_id: String,
    pub inverse_field_ids: Vec<String>,
    pub many: bool,
    pub nullable: bool,
    pub nullable_items: Option<bool>,
    pub unique: bool,
    pub min_refs: usize,
    pub max_refs: Option<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolverDoc {
    pub id: String,
    pub field_id: String,
    pub coordinate: String,
    pub description: Option<DescriptionDoc>,
    pub argument_ids: Vec<String>,
    pub returns: TypeRefDoc,
    pub scopes: Vec<ResolverScopeDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResolverScopeDoc {
    pub scope: ResolverScope,
    pub capability_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResolverScope {
    QueryRoot,
    MutationRoot,
    SubscriptionSetup,
    Nested,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CapabilityDoc {
    pub id: String,
    pub models: Vec<ModelCapabilityDoc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelCapabilityDoc {
    pub model_id: String,
    pub method_ids: Vec<String>,
}
