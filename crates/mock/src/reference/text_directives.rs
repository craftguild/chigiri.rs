//! Render the directive catalog and directive details in stable order.

use super::{InputKind, directive_types::*, man, text};
use std::io::{self, Write};

pub(super) fn catalog(values: &[DirectiveDoc], out: &mut impl Write) -> io::Result<()> {
    man::section(out, "DIRECTIVES")?;
    for directive in values {
        writeln!(
            out,
            "    @{} — {} [{} / {}]",
            directive.name,
            directive.purpose,
            directive.class.label(),
            directive.execution.label()
        )?;
        write!(out, "        ")?;
        text::visible(out, &directive.syntax)?;
        writeln!(out)?;
    }
    Ok(())
}

pub(super) fn topic_detail(
    value: &DirectiveTopicDoc,
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    detail_with_rules(&value.catalog_document(), &value.rules, input, out)
}

fn detail_with_rules(
    value: &DirectiveDoc,
    rules: &[DirectiveRuleTopicDoc],
    input: InputKind,
    out: &mut impl Write,
) -> io::Result<()> {
    super::input_text::detail_header(out, &value.purpose, input, "directives", &value.name)?;
    writeln!(
        out,
        "    @{}  {} / {}",
        value.name,
        value.class.label(),
        value.execution.label()
    )?;
    writeln!(out, "    Locations: {}", value.locations.join(" | "))?;
    man::section(out, "SYNTAX")?;
    write!(out, "    ")?;
    text::visible(out, &value.syntax)?;
    writeln!(out)?;
    if !value.arguments.is_empty() {
        man::section(out, "ARGUMENTS")?;
        for argument in &value.arguments {
            write!(
                out,
                "    {}: {}  {}",
                argument.name,
                argument.value_type,
                if argument.required {
                    "required"
                } else {
                    "optional"
                }
            )?;
            if let Some(default) = &argument.default {
                write!(out, "  default={}", text::json(default)?)?;
            }
            writeln!(out)?;
            if !argument.choices.is_empty() {
                writeln!(out, "        Choices: {}", argument.choices.join(" | "))?;
            }
        }
    }
    if let Some(description) = &value.declared_description {
        man::section(out, "SOURCE DESCRIPTION")?;
        write!(out, "The user SDL:")?;
        text::quoted(out, &description.text)?;
        writeln!(out)?;
    }
    if !value.examples.is_empty() {
        man::section(out, "EXAMPLES")?;
        for example in &value.examples {
            writeln!(
                out,
                "    {}: {}",
                if example.valid { "valid" } else { "invalid" },
                example.title
            )?;
            for line in example.sdl.lines() {
                writeln!(out, "        {line}")?;
            }
            writeln!(out, "        → {}", example.expected)?;
        }
    }
    if !value.rule_ids.is_empty() {
        man::section(out, "RULES")?;
        if rules.is_empty() {
            writeln!(out, "    {}", value.rule_ids.join(", "))?;
        } else {
            for rule in rules {
                writeln!(out, "    {} [{}]", rule.description, rule.id)?;
            }
        }
    }
    man::section(out, "SEE ALSO")?;
    for related in &value.related {
        writeln!(out, "    chigiri mock reference {related}")?;
    }
    writeln!(out, "    chigiri mock reference all")
}
