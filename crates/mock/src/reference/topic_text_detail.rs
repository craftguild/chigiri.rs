//! Render a stable human-readable topic list and the details of one selected item.

use super::{InputKind, directive_types::DirectiveTopicDoc, man, topic_types::*};
use std::io::{self, Write};

pub(super) fn render(
    content: &TopicContent,
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    match content {
        TopicContent::Directives(items) => directives(items, detail, input, out),
        TopicContent::Methods(items) => methods(items, detail, input, out),
        TopicContent::Hooks(items) => hooks(items, detail, input, out),
        TopicContent::Models(items) => models(items, detail, input, out),
        TopicContent::Types(items) => super::topic_text_schema::types(items, detail, input, out),
        TopicContent::Resolvers(items) => {
            super::topic_text_schema::resolvers(items, detail, input, out)
        }
        TopicContent::Limits(items) => limits(items, detail, out),
        TopicContent::Runtime(items) => runtime(items, detail, input, out),
        TopicContent::Consumers(items) => super::event_text::consumers(items, detail, input, out),
    }
}

fn runtime(
    items: &[RuntimeTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if !detail {
        for item in items {
            writeln!(out, "    {} — {}", item.name, item.purpose)?;
        }
        return Ok(());
    }
    let item = &items[0];
    super::input_text::detail_header(out, &item.purpose, input, "runtime", &item.name)?;
    man::section(out, "AVAILABILITY")?;
    for value in &item.availability {
        writeln!(out, "    {value}")?;
    }
    man::section(out, "SIGNATURES")?;
    for signature in &item.signatures {
        writeln!(out, "    {signature}")?;
    }
    man::section(out, "LIMITS")?;
    for limit in &item.limits {
        writeln!(out, "    {limit}")?;
    }
    man::section(out, "EXAMPLES")?;
    for example in &item.examples {
        writeln!(out, "    {}", example.title)?;
        for line in example.code.lines() {
            writeln!(out, "        {line}")?;
        }
    }
    man::see_also(out)
}

fn directives(
    items: &[DirectiveTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if detail {
        return super::text_directives::topic_detail(&items[0], input, out);
    }
    for item in items {
        writeln!(
            out,
            "    @{} — {} [{}]",
            item.name,
            item.purpose,
            item.class.label()
        )?;
    }
    Ok(())
}

fn methods(
    items: &[MethodTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if detail {
        return method(&items[0], input, out);
    }
    for item in items {
        writeln!(
            out,
            "    {} — {} [{}]",
            item.name, item.purpose, item.effect
        )?;
    }
    Ok(())
}

pub(super) fn method(
    item: &MethodTopicDoc,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    super::input_text::detail_header(out, &item.purpose, input, "methods", &item.name)?;
    writeln!(
        out,
        "Authorization: {} Eligibility: {}",
        item.effect, item.eligibility
    )?;
    man::section(out, "SIGNATURE")?;
    writeln!(out, "    {}", item.signature)?;
    if !item.options.is_empty() {
        man::section(out, "OPTIONS")?;
        for option in &item.options {
            writeln!(
                out,
                "    {}: {} nullable={} default={:?} max={:?}",
                option.name, option.value_type, option.nullable, option.default, option.maximum
            )?;
        }
    }
    if !item.rules.is_empty() {
        man::section(out, "SEMANTICS")?;
        for rule in &item.rules {
            writeln!(out, "    {rule}")?;
        }
    }
    if !item.examples.is_empty() {
        man::section(out, "EXAMPLES")?;
    }
    for example in &item.examples {
        writeln!(out, "    {}", example.title)?;
        for line in example.code.lines() {
            writeln!(out, "        {line}")?;
        }
    }
    man::section(out, "IDENTIFIERS")?;
    writeln!(out, "    {}", item.id)?;
    man::see_also(out)
}

fn hooks(
    items: &[HookTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if detail {
        return hook(&items[0], input, out);
    }
    for item in items {
        writeln!(
            out,
            "    {} — {} enabled={:?}",
            item.name, item.purpose, item.enabled
        )?;
    }
    Ok(())
}

pub(super) fn hook(item: &HookTopicDoc, input: InputKind, out: &mut impl Write) -> io::Result<()> {
    super::input_text::detail_header(out, &item.purpose, input, "hooks", &item.name)?;
    writeln!(out, "    item: {}", item.name)?;
    writeln!(
        out,
        "    module: {}  phase={} timing={} enabled={:?}",
        item.module_path, item.phase, item.timing, item.enabled
    )?;
    writeln!(
        out,
        "    statePatch={} reject={} models_read_only={}",
        item.state_patch, item.reject, item.models_read_only
    )?;
    writeln!(
        out,
        "    logging={} source_positions_nullable={} diagnostic_fields=[{}]",
        item.logging_format,
        item.source_positions_nullable,
        item.diagnostic_fields.join(", ")
    )?;
    man::section(out, "EXAMPLES")?;
    writeln!(out, "    {}", item.example.title)?;
    for line in item.example.code.lines() {
        writeln!(out, "        {line}")?;
    }
    man::see_also(out)
}

fn models(
    items: &[ModelTopicDoc],
    detail: bool,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    if items.is_empty() {
        return writeln!(
            out,
            "    No models are available. Specify input with --schema, --config, or --config-dir."
        );
    }
    if detail {
        return model(&items[0], input, out);
    }
    for item in items {
        writeln!(
            out,
            "    {} — methods: {}",
            item.name,
            item.methods.join(", ")
        )?;
    }
    Ok(())
}

pub(super) fn model(
    item: &ModelTopicDoc,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    super::input_text::detail_header(
        out,
        "Display one model derived from the schema input.",
        input,
        "models",
        &item.name,
    )?;
    if let Some(description) = &item.description {
        write!(out, "    ")?;
        super::text::visible(out, description)?;
        writeln!(out)?;
    }
    writeln!(
        out,
        "    visibility: {}\n    source: {}\n    key: {}\n    methods: {}",
        item.visibility,
        item.source.as_deref().unwrap_or("synthetic"),
        item.key.as_deref().unwrap_or("none"),
        item.methods.join(", ")
    )?;
    man::section(out, "FIELDS")?;
    for slot in &item.slots {
        writeln!(
            out,
            "    {}: {} visibility={} source={} required={} writable={} operators=[{}] sortable={}",
            slot.name,
            slot.value_type,
            slot.visibility,
            slot.source.as_deref().unwrap_or("synthetic"),
            slot.create_required,
            slot.writable,
            slot.operators.join(", "),
            slot.sortable
        )?;
        if let Some(description) = &slot.description {
            write!(out, "        description: ")?;
            super::text::visible(out, description)?;
            writeln!(out)?;
        }
    }
    if !item.relations.is_empty() {
        man::section(out, "RELATIONS")?;
        for relation in &item.relations {
            writeln!(
                out,
                "    {} -> {} many={} nullable={}",
                relation.field, relation.target, relation.many, relation.nullable
            )?;
        }
    }
    man::section(out, "IDENTIFIERS")?;
    writeln!(out, "    {}", item.id)?;
    man::see_also(out)
}

fn limits(items: &[LimitTopicDoc], detail: bool, out: &mut impl Write) -> io::Result<()> {
    if detail {
        return limit(&items[0], out);
    }
    for item in items {
        writeln!(
            out,
            "    {}: {} {} — {}",
            item.id, item.value, item.unit, item.purpose
        )?;
    }
    Ok(())
}

pub(super) fn limit(item: &LimitTopicDoc, out: &mut impl Write) -> io::Result<()> {
    man::identity(
        out,
        &item.purpose,
        "chigiri mock reference limits [OPTIONS]",
    )?;
    man::section(out, "DESCRIPTION")?;
    writeln!(
        out,
        "    item: {}\n    value: {} {}\n    scope: {}",
        item.id, item.value, item.unit, item.scope
    )?;
    man::see_also(out)
}
