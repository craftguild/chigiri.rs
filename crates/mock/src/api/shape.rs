//! Share closed predicate and sort-table shapes across decoding, query planning, and Luau references.

use chigiri::core::Object;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QueryType {
    FieldName,
    Operator,
    Operand,
    Predicates,
    Direction,
}

#[derive(Clone, Copy)]
pub(crate) struct Field {
    pub name: &'static str,
    pub ty: QueryType,
}

pub(crate) const FIELD: Field = Field {
    name: "field",
    ty: QueryType::FieldName,
};
pub(crate) const OP: Field = Field {
    name: "op",
    ty: QueryType::Operator,
};
pub(crate) const VALUE: Field = Field {
    name: "value",
    ty: QueryType::Operand,
};
pub(crate) const DIRECTION: Field = Field {
    name: "direction",
    ty: QueryType::Direction,
};

#[derive(Clone, Copy)]
pub(crate) struct Shape {
    pub id: &'static str,
    pub fields: &'static [Field],
}

impl Shape {
    pub fn accepts(self, object: &Object) -> bool {
        object.len() == self.fields.len()
            && self
                .fields
                .iter()
                .all(|field| object.contains_key(field.name))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Group {
    All,
    Any,
}

pub(crate) const GROUPS: &[(Group, Shape)] = &[
    (
        Group::All,
        Shape {
            id: "predicate_all",
            fields: &[Field {
                name: "all",
                ty: QueryType::Predicates,
            }],
        },
    ),
    (
        Group::Any,
        Shape {
            id: "predicate_any",
            fields: &[Field {
                name: "any",
                ty: QueryType::Predicates,
            }],
        },
    ),
];

pub(crate) const LEAF: Shape = Shape {
    id: "predicate_leaf",
    fields: &[FIELD, OP, VALUE],
};
pub(crate) const ORDER: Shape = Shape {
    id: "sort_key",
    fields: &[FIELD, DIRECTION],
};

pub(crate) fn predicate_field(name: &str) -> Option<QueryType> {
    GROUPS
        .iter()
        .flat_map(|(_, shape)| shape.fields)
        .chain(LEAF.fields)
        .find(|field| field.name == name)
        .map(|field| field.ty)
}

pub(crate) fn order_field(name: &str) -> Option<QueryType> {
    ORDER
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.ty)
}
