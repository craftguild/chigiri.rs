//! Render schema-derived topic details without dropping machine-visible contracts.

use super::{InputKind, man, schema_types::*, topic_schema_types::*};
use std::io::{self, Write};

pub(super) fn types(
    items: &[TypeTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(out, "    No schema types are available for this input.");
    }
    if detail {
        return type_detail(&items[0], input, out);
    }
    for item in items {
        writeln!(out, "    {} — {}", item.name, item.kind)?;
    }
    Ok(())
}

fn type_detail(item: &TypeTopicDoc, input: InputKind, out: &mut impl Write) -> io::Result<()> {
    super::input_text::detail_header(
        out,
        item.description
            .as_deref()
            .unwrap_or("GraphQL schema type."),
        input,
        "types",
        &item.name,
    )?;
    writeln!(
        out,
        "    kind={} model={} one_of={}",
        item.kind, item.model, item.one_of
    )?;
    if !item.interfaces.is_empty() {
        writeln!(out, "    interfaces: {}", item.interfaces.join(", "))?;
    }
    if !item.possible_types.is_empty() {
        writeln!(
            out,
            "    possible types: {}",
            item.possible_types.join(", ")
        )?;
    }
    if let Some(scalar) = &item.scalar {
        man::section(out, "REPRESENTATION")?;
        write!(out, "    type={}", representation(&scalar.representation))?;
        constraints(out, &scalar.constraints)?;
        writeln!(out)?;
    }
    if !item.fields.is_empty() {
        man::section(out, "FIELDS")?;
        for field in &item.fields {
            field_line(out, field)?;
        }
    }
    if !item.inputs.is_empty() {
        man::section(out, "INPUT FIELDS")?;
        for input in &item.inputs {
            write!(out, "    ")?;
            input_value(out, input)?;
            writeln!(out)?;
        }
    }
    if !item.enum_values.is_empty() {
        man::section(out, "ENUM VALUES")?;
        for value in &item.enum_values {
            write!(out, "    {}", value.name)?;
            description(out, value.description.as_deref())?;
            writeln!(out)?;
        }
    }
    man::section(out, "IDENTIFIERS")?;
    writeln!(out, "    {}", item.id)?;
    man::see_also(out)
}

fn field_line(out: &mut impl Write, field: &TypeFieldTopicDoc) -> io::Result<()> {
    write!(out, "    {}", field.name)?;
    if !field.arguments.is_empty() {
        write!(out, "(")?;
        for (index, argument) in field.arguments.iter().enumerate() {
            if index > 0 {
                write!(out, ", ")?;
            }
            input_value(out, argument)?;
        }
        write!(out, ")")?;
    }
    write!(out, ": {}", field.value_type)?;
    constraints(out, &field.constraints)?;
    role(out, &field.role)?;
    description(out, field.description.as_deref())?;
    writeln!(out)
}

fn input_value(out: &mut impl Write, input: &InputTopicDoc) -> io::Result<()> {
    write!(out, "{}: {}", input.name, input.value_type)?;
    if let Some(default) = &input.default {
        write!(out, " = ")?;
        literal(out, default)?;
    }
    write!(
        out,
        " required_after_coercion={}",
        input.required_after_coercion
    )?;
    constraints(out, &input.constraints)?;
    description(out, input.description.as_deref())
}

fn description(out: &mut impl Write, value: Option<&str>) -> io::Result<()> {
    if let Some(value) = value {
        write!(out, " — ")?;
        super::text::visible(out, value)?;
    }
    Ok(())
}

fn literal(out: &mut impl Write, value: &LiteralDoc) -> io::Result<()> {
    match value {
        LiteralDoc::Null => write!(out, "null"),
        LiteralDoc::Boolean(value) => write!(out, "{value}"),
        LiteralDoc::Integer(value) | LiteralDoc::Float(value) | LiteralDoc::Enum(value) => {
            super::text::visible(out, value)
        }
        LiteralDoc::String(value) => super::text::quoted(out, value),
        LiteralDoc::List(values) => {
            write!(out, "[")?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    write!(out, ", ")?;
                }
                literal(out, value)?;
            }
            write!(out, "]")
        }
        LiteralDoc::Object(fields) => {
            write!(out, "{{")?;
            for (index, field) in fields.iter().enumerate() {
                if index > 0 {
                    write!(out, ", ")?;
                }
                write!(out, "{}: ", field.name)?;
                literal(out, &field.value)?;
            }
            write!(out, "}}")
        }
    }
}

fn constraints(out: &mut impl Write, value: &ConstraintsDoc) -> io::Result<()> {
    let mut body = Vec::new();
    let mut first = true;
    macro_rules! number {
        ($field:ident, $name:literal) => {
            if let Some(value) = value.$field {
                separator(&mut body, &mut first)?;
                write!(body, "{}={}", $name, value)?;
            }
        };
    }
    macro_rules! text {
        ($field:ident, $name:literal) => {
            if let Some(value) = &value.$field {
                separator(&mut body, &mut first)?;
                write!(body, "{}=", $name)?;
                super::text::quoted(&mut body, value)?;
            }
        };
    }
    number!(exclusive_min, "exclusive_min");
    number!(exclusive_max, "exclusive_max");
    number!(multiple_of, "multiple_of");
    number!(min_length, "min_length");
    number!(max_length, "max_length");
    text!(starts_with, "starts_with");
    text!(ends_with, "ends_with");
    text!(contains, "contains");
    text!(not_contains, "not_contains");
    text!(pattern, "pattern");
    if let Some(format) = value.format {
        separator(&mut body, &mut first)?;
        write!(body, "format={}", format.as_str())?;
    }
    number!(min, "min");
    number!(max, "max");
    number!(min_bytes, "min_bytes");
    number!(max_bytes, "max_bytes");
    number!(min_items, "min_items");
    number!(max_items, "max_items");
    if !body.is_empty() {
        write!(out, " constraints(")?;
        out.write_all(&body)?;
        write!(out, ")")?;
    }
    Ok(())
}

fn separator(out: &mut impl Write, first: &mut bool) -> io::Result<()> {
    if !*first {
        write!(out, ", ")?;
    }
    *first = false;
    Ok(())
}

fn representation(value: &RepresentationDoc) -> &'static str {
    match value {
        RepresentationDoc::String => "STRING",
        RepresentationDoc::Boolean => "BOOLEAN",
        RepresentationDoc::Integer => "INTEGER",
        RepresentationDoc::Number => "NUMBER",
        RepresentationDoc::Object => "OBJECT",
        RepresentationDoc::Array => "ARRAY",
        RepresentationDoc::Json => "JSON",
    }
}

fn role(out: &mut impl Write, value: &TypeFieldRoleTopicDoc) -> io::Result<()> {
    match value {
        TypeFieldRoleTopicDoc::Stored {
            model,
            slot,
            model_key,
        } => write!(
            out,
            " role=stored(model={model}, slot={slot}, model_key={model_key})"
        ),
        TypeFieldRoleTopicDoc::Value => write!(out, " role=value"),
        TypeFieldRoleTopicDoc::Owner { relation } => relation_role(out, "owner", relation),
        TypeFieldRoleTopicDoc::Inverse { relation } => relation_role(out, "inverse", relation),
        TypeFieldRoleTopicDoc::Resolver {
            coordinate,
            module_path,
        } => write!(
            out,
            " role=resolver(coordinate={coordinate}, module={module_path})"
        ),
        TypeFieldRoleTopicDoc::Declaration => write!(out, " role=declaration"),
    }
}

fn relation_role(
    out: &mut impl Write,
    kind: &str,
    relation: &FieldRelationTopicDoc,
) -> io::Result<()> {
    write!(
        out,
        " role={kind}(owner={}.{}, target={}, storage_keys=[{}], reference_keys=[{}], inverse_fields=[{}], many={}, nullable={}, unique={}, min_refs={}, max_refs={:?})",
        relation.owner_model,
        relation.owner_field,
        relation.target_model,
        relation.storage_keys.join(", "),
        relation.reference_keys.join(", "),
        relation.inverse_fields.join(", "),
        relation.many,
        relation.nullable,
        relation.unique,
        relation.min_refs,
        relation.max_refs
    )
}

pub(super) fn resolvers(
    items: &[ResolverTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(out, "    No resolvers are available for this input.");
    }
    if detail {
        return resolver(&items[0], input, out);
    }
    for item in items {
        writeln!(out, "    {} — {}", item.coordinate, item.module_path)?;
    }
    Ok(())
}

fn resolver(item: &ResolverTopicDoc, input: InputKind, out: &mut impl Write) -> io::Result<()> {
    super::input_text::detail_header(
        out,
        "Resolver module contract derived from the schema.",
        input,
        "resolvers",
        &item.coordinate,
    )?;
    if let Some(description) = &item.description {
        write!(out, "    ")?;
        super::text::visible(out, description)?;
        writeln!(out)?;
    }
    man::section(out, "SIGNATURE")?;
    write!(out, "    {}(", item.coordinate)?;
    for (index, argument) in item.arguments.iter().enumerate() {
        if index > 0 {
            write!(out, ", ")?;
        }
        input_value(out, argument)?;
    }
    writeln!(out, "): {}", item.returns)?;
    man::section(out, "MODULE")?;
    writeln!(out, "    {}", item.module_path)?;
    if let Some(source) = &item.subscription_source {
        man::section(out, "SOURCES")?;
        writeln!(
            out,
            "    {}\n    {}\n    {}\n    {}",
            source.graphql_subscribe_signature,
            source.pubsub_signature,
            source.generator_signature,
            source.sequence_input_definition
        )?;
        writeln!(
            out,
            "    payload: {}; nominal source: {}",
            source.payload_type, source.source_type
        )?;
    }
    man::section(out, "CAPABILITIES")?;
    for scope in &item.scopes {
        writeln!(out, "    {}", scope.scope)?;
        for model in &scope.models {
            writeln!(out, "        {}: {}", model.model, model.methods.join(", "))?;
        }
    }
    man::section(out, "IDENTIFIERS")?;
    writeln!(out, "    {}", item.id)?;
    man::see_also(out)
}
