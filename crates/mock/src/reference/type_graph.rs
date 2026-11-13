//! Record shared API types in a closed graph and derive query tables from the runtime's validated shapes.

use super::{ReferenceDocument, ReferenceError, budget::Budget};
use crate::api::{
    self,
    shape::{self, QueryType},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApiTypeDoc {
    pub id: String,
    pub shape: TypeShapeDoc,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum TypeShapeDoc {
    Nil,
    Never,
    Void,
    HostFunction {
        calling_convention: CallingConvention,
        minimum_arity: usize,
        maximum_arity: usize,
        arguments: Vec<TypeFieldDoc>,
        returns_type_id: String,
    },
    Primitive {
        representation: Primitive,
        min_bytes: Option<usize>,
        max_bytes: Option<usize>,
    },
    ModelBinding {
        binding: ModelBinding,
        value_policy: Option<crate::api::model_values::Policy>,
        typename_field_name: Option<String>,
    },
    Array {
        item_type_id: String,
        dense: bool,
        maximum: Option<usize>,
    },
    Options {
        option_ids: Vec<String>,
        closed: bool,
    },
    Union {
        variant_type_ids: Vec<String>,
    },
    Object {
        fields: Vec<TypeFieldDoc>,
        closed: bool,
    },
    Enumeration {
        values: Vec<String>,
    },
    PredicateOperand {
        field_member: String,
        operator_member: String,
        operator_ids: Vec<String>,
        nullable: bool,
        apply_storage_constraints: bool,
    },
    NullSentinel {
        lua_nil: bool,
    },
    RelationTargetOptions {
        relation_argument: String,
        options_type_id: String,
    },
    ModelCollection {
        exact_graphql_names: bool,
        context_capability: bool,
    },
    Map {
        value_type_id: String,
        string_keys: bool,
    },
    Function {
        calling_convention: CallingConvention,
        arity: usize,
        argument_type_ids: Vec<String>,
        returns_type_id: String,
        returns_same_table: bool,
        json_marker: crate::api::context::JsonKind,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CallingConvention {
    Dot,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Primitive {
    String,
    Boolean,
    NonnegativeInteger,
    FiniteExactNumber,
    SafeInteger,
}

// Each binding name corresponds to the exact field name in `schema.models[].api_types`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ModelBinding {
    #[serde(rename = "record_slot_ids")]
    Record,
    #[serde(rename = "create_slot_ids")]
    Create,
    #[serde(rename = "update_slot_ids")]
    Update,
    #[serde(rename = "predicate_slot_ids")]
    Predicate,
    #[serde(rename = "relation_field_ids")]
    Relation,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypeFieldDoc {
    pub name: String,
    pub type_id: String,
    pub required: bool,
    pub nullable: bool,
}

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
) -> Result<(), ReferenceError> {
    use TypeShapeDoc as T;
    let mut push = |id: &str, shape| {
        budget.push(
            &mut doc.api_types,
            ApiTypeDoc {
                id: id.into(),
                shape,
            },
        )
    };
    for (id, representation, min_bytes, max_bytes) in [
        (
            "id",
            Primitive::String,
            Some(1),
            Some(crate::limits::ID_BYTES),
        ),
        ("boolean", Primitive::Boolean, None, None),
        ("number", Primitive::NonnegativeInteger, None, None),
        (
            "nonnegative_integer",
            Primitive::NonnegativeInteger,
            None,
            None,
        ),
    ] {
        push(
            id,
            T::Primitive {
                representation,
                min_bytes,
                max_bytes,
            },
        )?;
    }
    for (id, binding) in [
        ("record", ModelBinding::Record),
        ("create_input", ModelBinding::Create),
        ("update_input", ModelBinding::Update),
        ("relation", ModelBinding::Relation),
        ("field_name", ModelBinding::Predicate),
    ] {
        let shape = match binding {
            ModelBinding::Record => Some(api::model_values::Shape::Record),
            ModelBinding::Create => Some(api::model_values::Shape::Create),
            ModelBinding::Update => Some(api::model_values::Shape::Update),
            ModelBinding::Predicate | ModelBinding::Relation => None,
        };
        let value_policy = shape.map(|shape| shape.policy());
        let typename_field_name = value_policy
            .filter(|policy| policy.required_model_typename)
            .map(|_| api::model_values::TYPENAME.into());
        push(
            id,
            T::ModelBinding {
                binding,
                value_policy,
                typename_field_name,
            },
        )?;
    }
    for (id, item, maximum) in [
        ("records", "record", Some(crate::value::MAX_ITEMS)),
        ("predicates", "predicate", None),
        ("sort_keys", shape::ORDER.id, Some(api::query::SORT_KEYS)),
    ] {
        push(
            id,
            T::Array {
                item_type_id: item.into(),
                dense: true,
                maximum,
            },
        )?;
    }
    for (id, method) in [
        ("find_options", "model.find"),
        ("count_options", "model.count"),
    ] {
        push(
            id,
            T::Options {
                option_ids: doc
                    .options
                    .iter()
                    .filter(|option| option.method_id == method)
                    .map(|option| option.id.clone())
                    .collect(),
                closed: true,
            },
        )?;
    }
    push(
        "target_options",
        T::RelationTargetOptions {
            relation_argument: "relation".into(),
            options_type_id: "find_options".into(),
        },
    )?;
    push(
        "predicate",
        T::Union {
            variant_type_ids: shape::GROUPS
                .iter()
                .map(|(_, group)| group.id.into())
                .chain([shape::LEAF.id.into()])
                .collect(),
        },
    )?;
    for table in shape::GROUPS
        .iter()
        .map(|(_, shape)| *shape)
        .chain([shape::LEAF, shape::ORDER])
    {
        push(
            table.id,
            T::Object {
                fields: table
                    .fields
                    .iter()
                    .map(|field| TypeFieldDoc {
                        name: field.name.into(),
                        type_id: query_type(field.ty).into(),
                        required: true,
                        nullable: false,
                    })
                    .collect(),
                closed: true,
            },
        )?;
    }
    push(
        "operator",
        T::Enumeration {
            values: api::query::OPERATORS
                .iter()
                .map(|op| op.name.into())
                .collect(),
        },
    )?;
    push(
        "direction",
        T::Enumeration {
            values: api::query::DIRECTIONS
                .iter()
                .map(|(name, _)| (*name).into())
                .collect(),
        },
    )?;
    push(
        "operand",
        T::PredicateOperand {
            field_member: shape::FIELD.name.into(),
            operator_member: shape::OP.name.into(),
            operator_ids: doc
                .operators
                .iter()
                .map(|operator| operator.id.clone())
                .collect(),
            nullable: false,
            apply_storage_constraints: false,
        },
    )?;
    Ok(())
}

fn query_type(ty: QueryType) -> &'static str {
    match ty {
        QueryType::FieldName => "field_name",
        QueryType::Operator => "operator",
        QueryType::Operand => "operand",
        QueryType::Predicates => "predicates",
        QueryType::Direction => "direction",
    }
}
