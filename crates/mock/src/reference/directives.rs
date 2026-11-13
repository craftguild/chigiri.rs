//! Catalog standard Mock user directives from the completed semantic schema in stable order.

use super::{
    ReferenceDocument, ReferenceError, budget::Budget, directive_metadata, directive_types::*,
    schema_types::DescriptionDoc,
};
use crate::ir::{Format, MockIr};
use chigiri::core::Source;
use chigiri::core::language::ast::{DirectiveLocation, Value};
use chigiri::core::schema::{SemanticDirectiveDefinition, SemanticTypeRef, SemanticTypeRefKind};

const STANDARD: &[&str] = &["skip", "include", "deprecated", "specifiedBy", "oneOf"];
const MOCK: &[&str] = &["relation", "virtual", "constraint", "representation"];

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    ir: Option<&MockIr>,
) -> Result<(), ReferenceError> {
    let common = if ir.is_none() {
        Some(
            MockIr::parse(vec![Source::new(
                "reference.graphql",
                "type Query { reference: Boolean }",
            )])
            .map_err(|_| ReferenceError::contract())?,
        )
    } else {
        None
    };
    let semantic = ir
        .or(common.as_ref())
        .ok_or_else(ReferenceError::contract)?
        .graph()
        .source()
        .semantic();
    let ordered = STANDARD
        .iter()
        .chain(MOCK)
        .filter_map(|name| semantic.directive_definition(name))
        .chain(
            semantic
                .directive_definitions()
                .iter()
                .filter(|definition| {
                    !STANDARD.contains(&definition.name()) && !MOCK.contains(&definition.name())
                }),
        );
    for definition in ordered {
        budget.push(&mut doc.directives, build(semantic, definition)?)?;
    }
    Ok(())
}

fn build(
    schema: &chigiri::core::schema::SemanticSchema,
    definition: &SemanticDirectiveDefinition,
) -> Result<DirectiveDoc, ReferenceError> {
    let name = definition.name();
    let (class, execution) = if STANDARD.contains(&name) {
        (DirectiveClass::GraphqlStandard, DirectiveExecution::Graphql)
    } else if MOCK.contains(&name) {
        (DirectiveClass::ChigiriMock, DirectiveExecution::ChigiriMock)
    } else {
        (
            DirectiveClass::UserDeclared,
            DirectiveExecution::DeclarationOnly,
        )
    };
    let metadata = directive_metadata::get(name);
    let arguments = definition
        .arguments()
        .iter()
        .map(|argument| {
            Ok(DirectiveArgumentDoc {
                name: argument.name.clone(),
                value_type: type_name(&argument.ty),
                required: matches!(argument.ty.kind, SemanticTypeRefKind::NonNull(_))
                    && argument.default_value.is_none(),
                default: argument.default_value.as_ref().map(literal).transpose()?,
                choices: choices(schema, name, &argument.name),
            })
        })
        .collect::<Result<Vec<_>, ReferenceError>>()?;
    Ok(DirectiveDoc {
        id: format!("directive.{name}"),
        name: name.into(),
        class,
        execution,
        purpose: metadata.purpose.into(),
        declared_description: definition.description().map(|value| DescriptionDoc {
            origin: super::schema_types::DescriptionOrigin::UserSdl,
            text: value.value.value.clone(),
        }),
        repeatable: definition.is_repeatable(),
        locations: definition
            .locations()
            .iter()
            .map(|location| location_name(location.value).into())
            .collect(),
        syntax: syntax(definition),
        arguments,
        rule_ids: metadata.rules.iter().map(|value| (*value).into()).collect(),
        examples: metadata
            .examples
            .iter()
            .map(|value| value.document())
            .collect(),
        related: metadata
            .related
            .iter()
            .map(|value| (*value).into())
            .collect(),
    })
}

fn choices(
    schema: &chigiri::core::schema::SemanticSchema,
    directive: &str,
    argument: &str,
) -> Vec<String> {
    if (directive, argument) == ("representation", "type") {
        return schema
            .type_by_name("ChigiriMockRepresentation")
            .into_iter()
            .flat_map(|ty| ty.enum_values())
            .map(|value| value.name.value.clone())
            .collect();
    }
    if (directive, argument) == ("constraint", "format") {
        return Format::ALL
            .iter()
            .map(|value| value.as_str().into())
            .collect();
    }
    Vec::new()
}

fn type_name(reference: &SemanticTypeRef) -> String {
    match &reference.kind {
        SemanticTypeRefKind::Named(name) => name.clone(),
        SemanticTypeRefKind::List(item) => format!("[{}]", type_name(item)),
        SemanticTypeRefKind::NonNull(inner) => format!("{}!", type_name(inner)),
    }
}

fn syntax(definition: &SemanticDirectiveDefinition) -> String {
    let mut value = format!("directive @{}", definition.name());
    if !definition.arguments().is_empty() {
        value.push('(');
        for (index, argument) in definition.arguments().iter().enumerate() {
            if index > 0 {
                value.push_str(", ");
            }
            value.push_str(&argument.name);
            value.push_str(": ");
            value.push_str(&type_name(&argument.ty));
            if let Some(default) = &argument.default_value {
                value.push_str(" = ");
                value.push_str(&literal_text(default));
            }
        }
        value.push(')');
    }
    if definition.is_repeatable() {
        value.push_str(" repeatable");
    }
    value.push_str(" on ");
    value.push_str(
        &definition
            .locations()
            .iter()
            .map(|location| location_name(location.value))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    value
}

fn literal(value: &Value) -> Result<super::schema_types::LiteralDoc, ReferenceError> {
    super::schema_values::literal_value(value)
}

fn literal_text(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Boolean(value) => value.to_string(),
        Value::Int(value) | Value::Float(value) => value.clone(),
        Value::String(value) => {
            serde_json::to_string(&value.value).unwrap_or_else(|_| "\"\"".into())
        }
        Value::Enum(value) => value.value.clone(),
        Value::List(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| literal_text(&value.value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|field| format!("{}: {}", field.name.value, literal_text(&field.value.value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Variable(name) => format!("${}", name.value),
    }
}

fn location_name(value: DirectiveLocation) -> &'static str {
    match value {
        DirectiveLocation::Query => "QUERY",
        DirectiveLocation::Mutation => "MUTATION",
        DirectiveLocation::Subscription => "SUBSCRIPTION",
        DirectiveLocation::Field => "FIELD",
        DirectiveLocation::FragmentDefinition => "FRAGMENT_DEFINITION",
        DirectiveLocation::FragmentSpread => "FRAGMENT_SPREAD",
        DirectiveLocation::InlineFragment => "INLINE_FRAGMENT",
        DirectiveLocation::VariableDefinition => "VARIABLE_DEFINITION",
        DirectiveLocation::Schema => "SCHEMA",
        DirectiveLocation::Scalar => "SCALAR",
        DirectiveLocation::Object => "OBJECT",
        DirectiveLocation::FieldDefinition => "FIELD_DEFINITION",
        DirectiveLocation::ArgumentDefinition => "ARGUMENT_DEFINITION",
        DirectiveLocation::Interface => "INTERFACE",
        DirectiveLocation::Union => "UNION",
        DirectiveLocation::Enum => "ENUM",
        DirectiveLocation::EnumValue => "ENUM_VALUE",
        DirectiveLocation::InputObject => "INPUT_OBJECT",
        DirectiveLocation::InputFieldDefinition => "INPUT_FIELD_DEFINITION",
    }
}
