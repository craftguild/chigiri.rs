//! Carry expected decode shape forward without inferring it in reverse.

use chigiri::core::schema::TypeKind;

use super::JsonShape;
use crate::hooks::{StateFields, StateType};
use crate::ir::{MockIr, ModelId, Representation, TypeId, TypeRef, Wrapper};
use crate::value::ValueError;

#[derive(Clone, Copy)]
pub(super) enum Hint<'a> {
    Type(TypeId, &'a [Wrapper]),
    Record(ModelId),
    Query(bool),
    Predicate,
    Predicates,
    Operand,
    OrderKeys,
    OrderKey,
    Json(Option<JsonShape>),
    Typename,
    FieldName,
    State(&'a StateType),
    StateFields(&'a StateFields),
    HookReturn(&'a StateFields),
}

#[derive(Clone, Copy)]
pub(super) enum Fields<'a> {
    Type(TypeId),
    Record(ModelId),
    Query(bool),
    Predicate,
    OrderKey,
    Json,
    State(&'a StateFields),
    HookReturn(&'a StateFields),
}

pub(super) enum Container<'a> {
    Array(Hint<'a>),
    Object(Fields<'a>),
    Abstract(TypeId),
    Json,
}

impl<'a> Hint<'a> {
    pub(super) fn typed(ty: &'a TypeRef) -> Self {
        Self::Type(ty.named, &ty.wrappers)
    }

    pub(super) fn container(self, ir: &MockIr) -> Result<Container<'a>, ValueError> {
        match self {
            Self::StateFields(fields) => Ok(Container::Object(Fields::State(fields))),
            Self::HookReturn(fields) => Ok(Container::Object(Fields::HookReturn(fields))),
            Self::State(ty) => match ty {
                StateType::Nullable { of } => Self::State(of).container(ir),
                StateType::Object { fields } => Self::StateFields(fields).container(ir),
                StateType::List { of } => Ok(Container::Array(Self::State(of))),
                _ => Err(ValueError("primitive state value required")),
            },
            Self::Record(model) => Ok(Container::Object(Fields::Record(model))),
            Self::Query(count) => Ok(Container::Object(Fields::Query(count))),
            Self::Predicate => Ok(Container::Object(Fields::Predicate)),
            Self::Predicates => Ok(Container::Array(Self::Predicate)),
            Self::Operand => Ok(Container::Array(Self::Json(None))),
            Self::OrderKeys => Ok(Container::Array(Self::OrderKey)),
            Self::OrderKey => Ok(Container::Object(Fields::OrderKey)),
            Self::Json(Some(JsonShape::Object)) => Ok(Container::Object(Fields::Json)),
            Self::Json(Some(JsonShape::Array)) => Ok(Container::Array(Self::Json(None))),
            Self::Json(None) => Ok(Container::Json),
            Self::Typename | Self::FieldName => Err(ValueError("schema name string required")),
            Self::Type(named, mut wrappers) => {
                if wrappers.first() == Some(&Wrapper::NonNull) {
                    wrappers = &wrappers[1..];
                }
                if wrappers.first() == Some(&Wrapper::List) {
                    return Ok(Container::Array(Self::Type(named, &wrappers[1..])));
                }
                let node = ir
                    .graph()
                    .types()
                    .get(named.0)
                    .ok_or(ValueError("unknown value type"))?;
                match node.kind {
                    TypeKind::InputObject => Ok(Container::Object(Fields::Type(named))),
                    TypeKind::Object => Ok(Container::Object(
                        ir.model_for(named)
                            .map_or(Fields::Type(named), Fields::Record),
                    )),
                    TypeKind::Interface | TypeKind::Union => Ok(Container::Abstract(named)),
                    TypeKind::Scalar => {
                        match node.scalar.as_ref().map(|spec| spec.representation) {
                            Some(Representation::Object) => {
                                Self::Json(Some(JsonShape::Object)).container(ir)
                            }
                            Some(Representation::Array) => {
                                Self::Json(Some(JsonShape::Array)).container(ir)
                            }
                            Some(Representation::Json) => Ok(Container::Json),
                            _ => Err(ValueError("primitive scalar required")),
                        }
                    }
                    _ => Err(ValueError("primitive value required")),
                }
            }
        }
    }
}
