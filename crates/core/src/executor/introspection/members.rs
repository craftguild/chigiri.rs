//! Resolve input-value, enum-value, and directive-member fields.

use crate::language::ast::Field;
use crate::schema::introspection::model::DefaultValueDescriptor;
use crate::schema::{SemanticDirectiveDefinition, SemanticSchema};
use crate::{Object, Value};

use super::metadata::{deprecation, description, directive_location};
use super::model::{EnumValueView, FieldView, InputValueView, Node, Output, TypeView};
use super::resolve::{argument_bool, list, optional_string};
use super::type_view::{from_ast, from_descriptor};

pub(super) fn field<'a>(
    schema: &'a SemanticSchema,
    view: FieldView<'a>,
    field: &Field,
    arguments: &Object,
) -> Output<'a> {
    match field.name.value.as_str() {
        "name" => Output::Text(field_name(view)),
        "description" => optional_string(field_description(view)),
        "args" => {
            let include = argument_bool(arguments, "includeDeprecated");
            list(
                field_arguments(view)
                    .into_iter()
                    .filter(|value| include || !deprecation(input_directives(*value)).0)
                    .map(Node::InputValue),
            )
        }
        "type" => Output::Node(Node::Type(field_type(schema, view))),
        "isDeprecated" => Output::Leaf(Value::Boolean(field_deprecation(view).0)),
        "deprecationReason" => optional_string(field_deprecation(view).1),
        _ => Output::Null,
    }
}

pub(super) fn input<'a>(
    schema: &'a SemanticSchema,
    view: InputValueView<'a>,
    field: &Field,
) -> Output<'a> {
    match field.name.value.as_str() {
        "name" => Output::Text(input_name(view)),
        "description" => optional_string(input_description(view)),
        "type" => Output::Node(Node::Type(input_type(schema, view))),
        "defaultValue" => input_default(view),
        "isDeprecated" => Output::Leaf(Value::Boolean(deprecation(input_directives(view)).0)),
        "deprecationReason" => optional_string(deprecation(input_directives(view)).1),
        _ => Output::Null,
    }
}

pub(super) fn enumeration<'a>(view: EnumValueView<'a>, field: &Field) -> Output<'a> {
    let (name, description, directives) = match view {
        EnumValueView::Semantic(value) => (
            value.name.value.as_str(),
            description(value.description.as_ref()),
            value.directives.as_slice(),
        ),
        EnumValueView::Introspection(value) => (value.name, value.description, &[][..]),
    };
    match field.name.value.as_str() {
        "name" => Output::Text(name),
        "description" => optional_string(description),
        "isDeprecated" => Output::Leaf(Value::Boolean(deprecation(directives).0)),
        "deprecationReason" => optional_string(deprecation(directives).1),
        _ => Output::Null,
    }
}

pub(super) fn directive<'a>(
    view: &'a SemanticDirectiveDefinition,
    field: &Field,
    arguments: &Object,
) -> Output<'a> {
    match field.name.value.as_str() {
        "name" => Output::Text(view.name()),
        "description" => optional_string(description(view.description())),
        "isRepeatable" => Output::Leaf(Value::Boolean(view.is_repeatable())),
        "locations" => Output::List(
            view.locations()
                .iter()
                .map(|value| Output::Text(directive_location(value.value)))
                .collect(),
        ),
        "args" => {
            let include = argument_bool(arguments, "includeDeprecated");
            list(
                view.arguments()
                    .iter()
                    .filter(|value| {
                        include
                            || !deprecation(input_directives(InputValueView::Directive(value))).0
                    })
                    .map(|value| Node::InputValue(InputValueView::Directive(value))),
            )
        }
        _ => Output::Null,
    }
}

fn field_name(view: FieldView<'_>) -> &str {
    match view {
        FieldView::Semantic(value) => &value.name.value,
        FieldView::Introspection(value) => value.name,
    }
}

fn field_description(view: FieldView<'_>) -> Option<&str> {
    match view {
        FieldView::Semantic(value) => description(value.description.as_ref()),
        FieldView::Introspection(value) => value.description,
    }
}

fn field_arguments(view: FieldView<'_>) -> Vec<InputValueView<'_>> {
    match view {
        FieldView::Semantic(value) => value.arguments.iter().map(InputValueView::Ast).collect(),
        FieldView::Introspection(value) => value
            .arguments
            .iter()
            .map(InputValueView::Introspection)
            .collect(),
    }
}

fn field_type<'a>(schema: &'a SemanticSchema, view: FieldView<'a>) -> TypeView<'a> {
    match view {
        FieldView::Semantic(value) => from_ast(&value.ty, schema),
        FieldView::Introspection(value) => from_descriptor(&value.ty, schema),
    }
}

fn field_deprecation(view: FieldView<'_>) -> (bool, Option<&str>) {
    match view {
        FieldView::Semantic(value) => deprecation(&value.directives),
        FieldView::Introspection(_) => (false, None),
    }
}

fn input_name(view: InputValueView<'_>) -> &str {
    match view {
        InputValueView::Ast(value) => &value.name.value,
        InputValueView::Directive(value) => &value.name,
        InputValueView::Introspection(value) => value.name,
    }
}

fn input_description(view: InputValueView<'_>) -> Option<&str> {
    match view {
        InputValueView::Ast(value) => description(value.description.as_ref()),
        InputValueView::Directive(value) => value
            .source_definition
            .as_ref()
            .and_then(|value| description(value.description.as_ref())),
        InputValueView::Introspection(value) => value.description,
    }
}

fn input_type<'a>(schema: &'a SemanticSchema, view: InputValueView<'a>) -> TypeView<'a> {
    match view {
        InputValueView::Ast(value) => from_ast(&value.ty, schema),
        InputValueView::Directive(value) => super::model::semantic_type_ref(&value.ty, schema),
        InputValueView::Introspection(value) => from_descriptor(&value.ty, schema),
    }
}

fn input_default(view: InputValueView<'_>) -> Output<'_> {
    match view {
        InputValueView::Ast(value) => value
            .default_value
            .as_ref()
            .map_or(Output::Null, |value| Output::Default(&value.value)),
        InputValueView::Directive(value) => value
            .default_value
            .as_ref()
            .map_or(Output::Null, Output::Default),
        InputValueView::Introspection(value) => {
            value
                .default_value
                .map_or(Output::Null, |value| match value {
                    DefaultValueDescriptor::Boolean(value) => {
                        Output::Text(if value { "true" } else { "false" })
                    }
                })
        }
    }
}

fn input_directives(view: InputValueView<'_>) -> &[crate::language::ast::Directive] {
    match view {
        InputValueView::Ast(value) => &value.directives,
        InputValueView::Directive(value) => value
            .source_definition
            .as_ref()
            .map_or(&[], |value| value.directives.as_slice()),
        InputValueView::Introspection(_) => &[],
    }
}
