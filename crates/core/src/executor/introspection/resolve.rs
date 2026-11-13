//! Resolve introspection-object fields from semantic metadata.

use crate::language::ast::Field;
use crate::schema::introspection::model::DescriptorKind;
use crate::schema::{SemanticSchema, TypeKind};
use crate::{Object, Value};

use super::metadata::{
    deprecation, description, directive_string, exposed_type_by_name, exposed_types, kind_name,
};
use super::model::{EnumValueView, FieldView, InputValueView, NamedType, Node, Output, TypeView};

pub(super) fn root<'a>(
    schema: &'a SemanticSchema,
    field: &Field,
    arguments: &Object,
) -> Output<'a> {
    match field.name.value.as_str() {
        "__schema" => Output::Node(Node::Schema),
        "__type" => Output::optional_node(
            argument_string(arguments, "name").and_then(|name| exposed_named_node(schema, name)),
        ),
        _ => unreachable!("unexpected introspection root field"),
    }
}

fn exposed_named_node<'a>(schema: &'a SemanticSchema, name: &str) -> Option<Node<'a>> {
    if let Some(ty) = exposed_type_by_name(schema, name) {
        Some(Node::Type(TypeView::Named(NamedType::Semantic(ty))))
    } else {
        crate::schema::introspection::type_by_name(name)
            .map(|ty| Node::Type(TypeView::Named(NamedType::Introspection(ty))))
    }
}

pub(super) fn node<'a>(
    schema: &'a SemanticSchema,
    parent: Node<'a>,
    field: &Field,
    arguments: &Object,
) -> Output<'a> {
    if field.name.value == "__typename" {
        return Output::Text(parent.runtime_type());
    }
    match parent {
        Node::Schema => schema_field(schema, field),
        Node::Type(view) => type_field(schema, view, field, arguments),
        Node::Field(view) => super::members::field(schema, view, field, arguments),
        Node::InputValue(view) => super::members::input(schema, view, field),
        Node::EnumValue(view) => super::members::enumeration(view, field),
        Node::Directive(view) => super::members::directive(view, field, arguments),
    }
}

fn schema_field<'a>(schema: &'a SemanticSchema, field: &Field) -> Output<'a> {
    match field.name.value.as_str() {
        "description" => optional_string(description(schema.description())),
        "types" => list(
            exposed_types(schema)
                .into_iter()
                .map(|ty| Node::Type(TypeView::Named(NamedType::Semantic(ty))))
                .chain(
                    crate::schema::introspection::types()
                        .iter()
                        .map(|ty| Node::Type(TypeView::Named(NamedType::Introspection(ty)))),
                ),
        ),
        "queryType" => root_type(schema, schema.roots().query()),
        "mutationType" => root_type(schema, schema.roots().mutation()),
        "subscriptionType" => root_type(schema, schema.roots().subscription()),
        "directives" => list(schema.directive_definitions().iter().map(Node::Directive)),
        _ => Output::Null,
    }
}

fn root_type<'a>(
    schema: &'a SemanticSchema,
    root: Option<&crate::schema::RootOperation>,
) -> Output<'a> {
    Output::optional_node(root.and_then(|root| named_node(schema, &root.type_name)))
}

fn type_field<'a>(
    schema: &'a SemanticSchema,
    view: TypeView<'a>,
    field: &Field,
    arguments: &Object,
) -> Output<'a> {
    match field.name.value.as_str() {
        "kind" => Output::Text(type_kind(&view)),
        "name" => optional_string(type_name(&view)),
        "description" => optional_string(type_description(&view)),
        "specifiedByURL" => optional_string(specified_by(&view)),
        "fields" => type_fields(view, arguments),
        "interfaces" => type_interfaces(schema, view),
        "possibleTypes" => possible_types(schema, view),
        "enumValues" => enum_values(view, arguments),
        "inputFields" => input_values(view, arguments),
        "ofType" => match view {
            TypeView::List(inner) | TypeView::NonNull(inner) => Output::Node(Node::Type(*inner)),
            TypeView::Named(_) => Output::Null,
        },
        "isOneOf" => is_one_of(view),
        _ => Output::Null,
    }
}

fn type_fields<'a>(view: TypeView<'a>, arguments: &Object) -> Output<'a> {
    let include = argument_bool(arguments, "includeDeprecated");
    match view {
        TypeView::Named(NamedType::Semantic(ty))
            if matches!(ty.kind(), TypeKind::Object | TypeKind::Interface) =>
        {
            list(
                ty.fields()
                    .iter()
                    .filter(|value| include || !deprecation(&value.directives).0)
                    .map(|value| Node::Field(FieldView::Semantic(value))),
            )
        }
        TypeView::Named(NamedType::Introspection(ty)) if ty.kind == DescriptorKind::Object => list(
            ty.fields
                .iter()
                .map(|value| Node::Field(FieldView::Introspection(value))),
        ),
        _ => Output::Null,
    }
}

fn type_interfaces<'a>(schema: &'a SemanticSchema, view: TypeView<'a>) -> Output<'a> {
    match view {
        TypeView::Named(NamedType::Semantic(ty))
            if matches!(ty.kind(), TypeKind::Object | TypeKind::Interface) =>
        {
            list(
                ty.implemented_interfaces()
                    .iter()
                    .filter_map(|name| named_node(schema, &name.value)),
            )
        }
        _ => Output::Null,
    }
}

fn possible_types<'a>(schema: &'a SemanticSchema, view: TypeView<'a>) -> Output<'a> {
    let TypeView::Named(NamedType::Semantic(ty)) = view else {
        return Output::Null;
    };
    match ty.kind() {
        TypeKind::Interface => list(
            schema
                .types()
                .iter()
                .filter(|candidate| {
                    candidate.kind() == TypeKind::Object
                        && candidate.implements_interface(ty.name())
                })
                .map(|candidate| Node::Type(TypeView::Named(NamedType::Semantic(candidate)))),
        ),
        TypeKind::Union => list(
            ty.union_members()
                .iter()
                .filter_map(|member| named_node(schema, &member.value)),
        ),
        _ => Output::Null,
    }
}

fn enum_values<'a>(view: TypeView<'a>, arguments: &Object) -> Output<'a> {
    let include = argument_bool(arguments, "includeDeprecated");
    match view {
        TypeView::Named(NamedType::Semantic(ty)) if ty.kind() == TypeKind::Enum => list(
            ty.enum_values()
                .iter()
                .filter(|value| include || !deprecation(&value.directives).0)
                .map(|value| Node::EnumValue(EnumValueView::Semantic(value))),
        ),
        TypeView::Named(NamedType::Introspection(ty)) if ty.kind == DescriptorKind::Enum => list(
            ty.enum_values
                .iter()
                .map(|value| Node::EnumValue(EnumValueView::Introspection(value))),
        ),
        _ => Output::Null,
    }
}

fn input_values<'a>(view: TypeView<'a>, arguments: &Object) -> Output<'a> {
    let include = argument_bool(arguments, "includeDeprecated");
    match view {
        TypeView::Named(NamedType::Semantic(ty)) if ty.kind() == TypeKind::InputObject => list(
            ty.input_fields()
                .iter()
                .filter(|value| include || !deprecation(&value.directives).0)
                .map(|value| Node::InputValue(InputValueView::Ast(value))),
        ),
        _ => Output::Null,
    }
}

fn is_one_of(view: TypeView<'_>) -> Output<'_> {
    match view {
        TypeView::Named(NamedType::Semantic(ty)) if ty.kind() == TypeKind::InputObject => {
            Output::Leaf(Value::Boolean(
                ty.directives()
                    .iter()
                    .any(|value| value.name.value == "oneOf"),
            ))
        }
        _ => Output::Null,
    }
}

fn named_node<'a>(schema: &'a SemanticSchema, name: &str) -> Option<Node<'a>> {
    if let Some(ty) = schema.type_by_name(name) {
        Some(Node::Type(TypeView::Named(NamedType::Semantic(ty))))
    } else {
        crate::schema::introspection::type_by_name(name)
            .map(|ty| Node::Type(TypeView::Named(NamedType::Introspection(ty))))
    }
}

fn type_kind(view: &TypeView<'_>) -> &'static str {
    match view {
        TypeView::List(_) => "LIST",
        TypeView::NonNull(_) => "NON_NULL",
        TypeView::Named(NamedType::Semantic(ty)) => kind_name(ty.kind()),
        TypeView::Named(NamedType::Introspection(ty)) => match ty.kind {
            DescriptorKind::Object => "OBJECT",
            DescriptorKind::Enum => "ENUM",
        },
    }
}

fn type_name<'a>(view: &TypeView<'a>) -> Option<&'a str> {
    match view {
        TypeView::Named(value) => Some(value.name_ref()),
        TypeView::List(_) | TypeView::NonNull(_) => None,
    }
}

fn type_description<'a>(view: &TypeView<'a>) -> Option<&'a str> {
    match view {
        TypeView::Named(NamedType::Semantic(ty)) => description(ty.description()),
        TypeView::Named(NamedType::Introspection(ty)) => ty.description,
        TypeView::List(_) | TypeView::NonNull(_) => None,
    }
}

fn specified_by<'a>(view: &TypeView<'a>) -> Option<&'a str> {
    match view {
        TypeView::Named(NamedType::Semantic(ty)) if ty.kind() == TypeKind::Scalar => {
            directive_string(ty.directives(), "specifiedBy", "url")
        }
        _ => None,
    }
}

pub(super) fn list<'a>(nodes: impl IntoIterator<Item = Node<'a>>) -> Output<'a> {
    Output::List(nodes.into_iter().map(Output::Node).collect())
}

pub(super) fn optional_string(value: Option<&str>) -> Output<'_> {
    value.map_or(Output::Null, Output::Text)
}

pub(super) fn argument_bool(arguments: &Object, name: &str) -> bool {
    matches!(arguments.get(name), Some(Value::Boolean(true)))
}

fn argument_string<'a>(arguments: &'a Object, name: &str) -> Option<&'a str> {
    match arguments.get(name) {
        Some(Value::String(value)) => Some(value),
        _ => None,
    }
}
