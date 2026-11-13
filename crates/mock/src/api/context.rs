//! Define context member names and JSON constructors once for every generated and runtime surface.

pub(crate) const SCOPE: &str = "scope";
pub(crate) const NULL: &str = "null";
pub(crate) const MODELS: &str = "models";
pub(crate) const REQUEST: &str = "request";
pub(crate) const FAIL: &str = "fail";
pub(crate) const READ: &str = "read";
pub(crate) const MUTATION: &str = "mutation";

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JsonKind {
    Object,
    Array,
}

impl JsonKind {
    pub fn luau(self) -> &'static str {
        match self {
            Self::Object => "ChigiriJsonObject",
            Self::Array => "ChigiriJsonArray",
        }
    }
    pub fn type_id(self) -> &'static str {
        match self {
            Self::Object => "json_object",
            Self::Array => "json_array",
        }
    }
}

pub(crate) struct Constructor {
    pub id: &'static str,
    pub name: &'static str,
    pub arity: usize,
    pub shape: JsonKind,
}

pub(crate) const CONSTRUCTORS: &[Constructor] = &[
    Constructor {
        id: "json_object_constructor",
        name: "jsonObject",
        arity: 1,
        shape: JsonKind::Object,
    },
    Constructor {
        id: "json_array_constructor",
        name: "jsonArray",
        arity: 1,
        shape: JsonKind::Array,
    },
];
