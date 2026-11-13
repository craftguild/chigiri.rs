//! Render the full human-readable reference with visible control characters.

use super::{ReferenceDocument, api_types::MethodDoc, man};
use crate::api::{Effect, TypeExpr};
use serde::Serialize;
use std::io::{self, Write};

pub(super) fn render(doc: &ReferenceDocument, out: &mut impl Write) -> io::Result<()> {
    man::title(out)?;
    man::identity(
        out,
        "Display the complete Mock API and loaded-schema reference.",
        super::input_text::selection_synopsis(doc.input_kind, "all", None),
    )?;
    man::section(out, "DESCRIPTION")?;
    writeln!(
        out,
        "    API: {}  input format: {}  JSON: {}",
        doc.api_version,
        doc.mock_format.as_deref().unwrap_or("null"),
        doc.format
    )?;
    writeln!(
        out,
        "    input_kind: {}  config_executed: {}",
        doc.input_kind.name(),
        doc.config_executed
    )?;
    writeln!(out, "    {}", super::input_text::boundary(doc.input_kind))?;
    writeln!(
        out,
        "    supported project: {}  bundle: {}",
        doc.supported_formats.project, doc.supported_formats.bundle
    )?;
    writeln!(
        out,
        "    binary: {}  luaur: {}",
        doc.binary.version, doc.binary.luaur
    )?;
    writeln!(out, "readiness_checked: false")?;
    super::text_directives::catalog(&doc.directives, out)?;
    man::section(out, "METHODS")?;
    writeln!(out, "M denotes a concrete model from the loaded schema.\n")?;
    for method in &doc.methods {
        write!(out, "    ")?;
        signature(out, method, "M")?;
        writeln!(
            out,
            "{} / receiver input arity {} / qualification {}",
            effect(method.effect),
            method.arity_including_receiver,
            json(&method.model_eligibility)?
        )?;
        for rule in doc
            .rules
            .iter()
            .filter(|rule| method.rule_ids.contains(&rule.id))
        {
            writeln!(out, "        {} [{}]", rule.description, rule.id)?;
        }
    }
    man::section(out, "OPTIONS")?;
    writeln!(
        out,
        "Options tables are closed unless their type says otherwise.\n"
    )?;
    for option in &doc.options {
        writeln!(
            out,
            "    {}.{}: {}  null={}  default={}  max={}",
            option.method_id,
            option.name,
            json(&option.value_type)?,
            option.nullable,
            json(&option.default)?,
            json(&option.maximum)?
        )?;
    }
    man::section(out, "API TYPES")?;
    writeln!(
        out,
        "Model-specific bindings are listed under schema.models[].api_types.\n"
    )?;
    for ty in &doc.api_types {
        writeln!(out, "    {}: {}", ty.id, json(&ty.shape)?)?;
    }
    man::section(out, "OPERATORS")?;
    for operator in &doc.operators {
        writeln!(
            out,
            "    {}: eligibility={} operand={}",
            operator.name,
            json(&operator.eligibility)?,
            json(&operator.operand)?
        )?;
    }
    writeln!(
        out,
        "    sort direction: {}",
        doc.sort_directions.join(", ")
    )?;
    man::section(out, "CONTEXT")?;
    for context in &doc.contexts {
        writeln!(
            out,
            "    {}: {} type={} scope={} [{}]",
            context.id,
            effect(context.effect),
            context.type_id,
            context.scope_value,
            context.method_ids.join(", ")
        )?;
    }
    writeln!(out, "    globals: {}", doc.globals.join(", "))?;
    man::section(out, "HOOKS")?;
    writeln!(
        out,
        "state_configured: {} (initial and runtime values are not disclosed)",
        doc.state_configured
    )?;
    for hook in &doc.hooks {
        writeln!(
            out,
            "    {} [{}] enabled={} context={} return={} statePatch={} reject={} models=read-only",
            hook.slot.name(),
            hook.module_path,
            json(&hook.enabled)?,
            hook.context_type_id,
            hook.result_type_id,
            hook.state_patch,
            hook.reject
        )?;
    }
    writeln!(
        out,
        "    rejection: {}\n    logging: {}",
        json(&doc.rejection)?,
        json(&doc.logging)?
    )?;
    super::event_text::full(&doc.events, doc.input_kind, out)?;
    man::section(out, "SEMANTICS AND ERRORS")?;
    for rule in &doc.rules {
        writeln!(
            out,
            "    {} [{}]\n        {}",
            rule.id,
            json(&rule.failure_class)?,
            rule.description
        )?;
    }
    man::section(out, "LIMITS")?;
    for limit in &doc.limits {
        writeln!(
            out,
            "    {}: {} {} / {} — {}",
            limit.id,
            limit.value,
            json(&limit.unit)?,
            limit.shared_scope,
            limit.description
        )?;
    }
    if let Some(schema) = &doc.schema {
        super::text_schema::render(schema, &doc.methods, out)?;
    }
    man::section(out, "EXAMPLES")?;
    writeln!(
        out,
        "Examples use generic User and Post names; they do not describe the loaded schema.\n"
    )?;
    for example in &doc.examples {
        writeln!(
            out,
            "    {} [{} / {}]",
            example.id,
            json(&example.origin)?,
            effect(example.scope)
        )?;
        write!(out, "    ")?;
        quoted(out, &example.title)?;
        writeln!(out)?;
        for line in example.luau.lines() {
            writeln!(out, "        {line}")?;
        }
        writeln!(out)?;
    }
    man::see_also(out)
}

pub(super) fn effect(effect: Effect) -> &'static str {
    match effect {
        Effect::Read => "read",
        Effect::Write => "mutation root or named consumer write",
    }
}

pub(super) fn signature(out: &mut impl Write, method: &MethodDoc, model: &str) -> io::Result<()> {
    write!(out, "ctx.models[")?;
    quoted(out, model)?;
    write!(out, "]:{}(", method.name)?;
    for (index, argument) in method.arguments.iter().enumerate() {
        if index != 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}: {}", argument.name, ty(argument.value_type))?;
    }
    writeln!(out, ") -> {}", ty(method.returns))
}

fn ty(ty: TypeExpr) -> &'static str {
    match ty {
        TypeExpr::FindOptions => "FindOptions<M>",
        TypeExpr::CountOptions => "CountOptions<M>",
        TypeExpr::TargetOptions => "FindOptions<Target<M, relation>>",
        TypeExpr::Relation => "Relation<M>",
        TypeExpr::CreateInput => "CreateInput<M>",
        TypeExpr::UpdateInput => "UpdateInput<M>",
        TypeExpr::Record => "Record<M>",
        TypeExpr::Records => "{Record<M>}",
        TypeExpr::Number => "number",
    }
}

pub(super) fn json(value: &impl Serialize) -> io::Result<String> {
    // Enum and constant vocabulary is already closed; escape controls for text output.
    let serialized = serde_json::to_string(value).map_err(io::Error::other)?;
    let mut result = Vec::with_capacity(serialized.len());
    visible(&mut result, &serialized)?;
    String::from_utf8(result).map_err(io::Error::other)
}

pub(super) fn visible(out: &mut impl Write, value: &str) -> io::Result<()> {
    for ch in value.chars() {
        if requires_visible_escape(ch) {
            write!(out, "\\u{{{:x}}}", ch as u32)?;
        } else {
            write!(out, "{ch}")?;
        }
    }
    Ok(())
}

pub(super) fn quoted(out: &mut impl Write, value: &str) -> io::Result<()> {
    write!(out, "\"")?;
    for ch in value.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            ch if requires_visible_escape(ch) => write!(out, "\\u{{{:x}}}", ch as u32)?,
            ch => write!(out, "{ch}")?,
        }
    }
    write!(out, "\"")
}

fn requires_visible_escape(ch: char) -> bool {
    ch.is_control() || is_bidi_control(ch) || matches!(ch, '\u{2028}' | '\u{2029}')
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}
