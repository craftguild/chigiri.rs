//! Render the schema reference graph with stable IDs and quoted SDL descriptions.

use super::{
    api_types::MethodDoc,
    man,
    schema_types::*,
    text::{json, quoted, signature},
};
use std::io::{self, Write};

pub(super) fn render(
    schema: &SchemaDoc,
    methods: &[MethodDoc],
    out: &mut impl Write,
) -> io::Result<()> {
    man::section(out, "SCHEMA")?;
    writeln!(out, "    digest: {}", schema.digest)?;
    writeln!(out, "    vocabulary: {}", schema.vocabulary)?;
    description(out, schema.description.as_ref())?;
    writeln!(
        out,
        "  Query: {}  Mutation: {}",
        schema.query_type_id,
        schema.mutation_type_id.as_deref().unwrap_or("none")
    )?;
    man::section(out, "MODELS")?;
    writeln!(
        out,
        "M denotes the current model in each generic API type below.\n"
    )?;
    for model in &schema.models {
        let ty = lookup(schema, &model.type_id)?;
        write!(out, "  {} M = ", model.id)?;
        quoted(out, &ty.name)?;
        writeln!(
            out,
            " / key={}",
            model.key_field_id.as_deref().unwrap_or("none")
        )?;
        description(out, model.description.as_ref())?;
        for method in methods
            .iter()
            .filter(|method| model.method_ids.contains(&method.id))
        {
            write!(out, "    ")?;
            signature(out, method, &ty.name)?;
        }
        writeln!(
            out,
            "Record<M>: all slots below; nullable slots may be omitted or set to ctx.null."
        )?;
        writeln!(
            out,
            "CreateInput<M>: writable slots; create_required=true slots are mandatory."
        )?;
        writeln!(out, "UpdateInput<M>: any subset of writable slots.")?;
        for slot in schema.slots.iter().filter(|slot| slot.model_id == model.id) {
            write!(out, "    {} ", slot.id)?;
            quoted(out, &slot.name)?;
            write!(out, ": ")?;
            reference(out, schema, &slot.value_type)?;
            writeln!(
                out,
                " / key={} create_required={} writable={}",
                slot.model_key, slot.create_required, slot.writable
            )?;
            description(out, slot.description.as_ref())?;
            write!(out, "      constraints=")?;
            wire(out, &slot.constraints)?;
            writeln!(
                out,
                " / operators=[{}] sortable={}",
                slot.filter_operator_ids.join(", "),
                slot.sortable
            )?;
        }
        write!(out, "    Relation<M>: ")?;
        for (index, field_id) in model.api_types.relation_field_ids.iter().enumerate() {
            if index != 0 {
                write!(out, " | ")?;
            }
            let field = schema
                .fields
                .iter()
                .find(|field| &field.id == field_id)
                .ok_or_else(invalid)?;
            quoted(out, &field.name)?;
        }
        writeln!(out)?;
    }
    man::section(out, "RELATIONS")?;
    writeln!(
        out,
        "min_refs and max_refs bound target references per source record.\n"
    )?;
    for relation in &schema.relations {
        write!(out, "  ")?;
        wire(out, relation)?;
        writeln!(out)?;
    }
    man::section(out, "SCHEMA TYPES")?;
    writeln!(
        out,
        "Wrappers use GraphQL notation; parenthesized IDs refer to this graph.\n"
    )?;
    for ty in &schema.types {
        write!(out, "  {} ", ty.id)?;
        quoted(out, &ty.name)?;
        writeln!(out, " {} one_of={}", json(&ty.kind)?, ty.one_of)?;
        description(out, ty.description.as_ref())?;
        writeln!(
            out,
            "    fields=[{}] inputs=[{}]",
            ty.field_ids.join(", "),
            ty.input_ids.join(", ")
        )?;
        writeln!(
            out,
            "    interfaces=[{}] possible_types=[{}]",
            ty.interface_ids.join(", "),
            ty.possible_type_ids.join(", ")
        )?;
        if let Some(scalar) = &ty.scalar {
            write!(out, "    scalar=")?;
            wire(out, scalar)?;
            writeln!(out)?;
        }
        for value in &ty.enum_values {
            write!(out, "    enum ")?;
            quoted(out, &value.name)?;
            writeln!(out)?;
            description(out, value.description.as_ref())?;
        }
    }
    man::section(out, "FIELDS AND INPUTS")?;
    for field in &schema.fields {
        write!(out, "  {} {}.", field.id, field.parent_type_id)?;
        quoted(out, &field.name)?;
        write!(out, ": ")?;
        reference(out, schema, &field.value_type)?;
        writeln!(out, " args=[{}]", field.argument_ids.join(", "))?;
        description(out, field.description.as_ref())?;
        write!(out, "    role=")?;
        wire(out, &field.role)?;
        write!(out, " constraints=")?;
        wire(out, &field.constraints)?;
        writeln!(out)?;
    }
    for input in &schema.inputs {
        write!(out, "  {} ", input.id)?;
        quoted(out, &input.name)?;
        write!(out, ": ")?;
        reference(out, schema, &input.value_type)?;
        writeln!(
            out,
            " required_after_coercion={}",
            input.required_after_coercion
        )?;
        description(out, input.description.as_ref())?;
        write!(out, "    default=")?;
        wire(out, &input.default)?;
        write!(out, " constraints=")?;
        wire(out, &input.constraints)?;
        writeln!(out)?;
    }
    man::section(out, "RESOLVERS")?;
    writeln!(
        out,
        "Each resolver lists its return type and scope-specific capability.\n"
    )?;
    for resolver in &schema.resolvers {
        write!(out, "  {} ", resolver.id)?;
        quoted(out, &resolver.coordinate)?;
        write!(out, " args=[{}] -> ", resolver.argument_ids.join(", "))?;
        reference(out, schema, &resolver.returns)?;
        writeln!(out)?;
        description(out, resolver.description.as_ref())?;
        for scope in &resolver.scopes {
            writeln!(
                out,
                "    {} -> {}",
                json(&scope.scope)?,
                scope.capability_id
            )?;
        }
    }
    for capability in &schema.capabilities {
        writeln!(out, "  {}", capability.id)?;
        for model in &capability.models {
            writeln!(
                out,
                "    {} [{}]",
                model.model_id,
                model.method_ids.join(", ")
            )?;
        }
    }
    Ok(())
}

fn description(out: &mut impl Write, value: Option<&DescriptionDoc>) -> io::Result<()> {
    if let Some(value) = value {
        write!(out, "    user_sdl description: ")?;
        quoted(out, &value.text)?;
        writeln!(out)?;
    }
    Ok(())
}

pub(super) fn reference(
    out: &mut impl Write,
    schema: &SchemaDoc,
    ty: &TypeRefDoc,
) -> io::Result<()> {
    for wrapper in &ty.wrappers {
        if matches!(wrapper, WrapperDoc::List) {
            write!(out, "[")?;
        }
    }
    quoted(out, &lookup(schema, &ty.type_id)?.name)?;
    for wrapper in ty.wrappers.iter().rev() {
        write!(
            out,
            "{}",
            match wrapper {
                WrapperDoc::NonNull => "!",
                WrapperDoc::List => "]",
            }
        )?;
    }
    write!(out, " ({})", ty.type_id)
}

fn lookup<'a>(schema: &'a SchemaDoc, id: &str) -> io::Result<&'a TypeDoc> {
    id.strip_prefix("type/")
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| schema.types.get(index))
        .filter(|ty| ty.id == id)
        .ok_or_else(invalid)
}

fn invalid() -> io::Error {
    io::Error::other("invalid reference graph")
}

fn wire(out: &mut impl Write, value: &impl serde::Serialize) -> io::Result<()> {
    // JSON strings may contain C1 or bidirectional controls, so text output
    // escapes every control character before displaying user-owned content.
    serde_json::to_writer(Escaped(out), value).map_err(io::Error::other)
}

struct Escaped<'a, W>(&'a mut W);
impl<W: Write> Write for Escaped<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // serde_json writes strings at UTF-8 boundaries.
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        for ch in text.chars() {
            if ch.is_control()
                || matches!(ch, '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                write!(self.0, "\\u{{{:x}}}", ch as u32)?;
            } else {
                write!(self.0, "{ch}")?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
