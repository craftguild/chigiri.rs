//! Describe reference input authority consistently across human-readable views.

use super::InputKind;
use std::io::{self, Write};

pub(super) fn boundary(input: InputKind) -> &'static str {
    match input {
        InputKind::Common => {
            "Common input is schema-independent; runtime readiness was not checked."
        }
        InputKind::Schema => {
            "Schema input uses explicitly supplied live SDL and does not read or update a project; runtime readiness was not checked."
        }
        InputKind::Project => {
            "Project input uses adopted public SDL plus current project-local storage; external public-source freshness and runtime readiness were not checked."
        }
        InputKind::Bundle => {
            "Bundle input uses SDL embedded in the portable bundle; external-source freshness and runtime readiness were not checked."
        }
    }
}

pub(super) fn selection_synopsis(input: InputKind, topic: &str, name: Option<&str>) -> String {
    let name = name.map_or(String::new(), |name| format!(" {name}"));
    format!(
        "chigiri mock reference {topic}{name}{} [OPTIONS]",
        input_option(input)
    )
}

pub(super) fn detail_header(
    out: &mut impl Write,
    purpose: &str,
    input: InputKind,
    topic: &str,
    name: &str,
) -> io::Result<()> {
    super::man::identity(out, purpose, selection_synopsis(input, topic, Some(name)))?;
    super::man::section(out, "DESCRIPTION")?;
    writeln!(out, "    {}", boundary(input))
}

pub(super) fn landing_synopsis(input: InputKind) -> String {
    format!(
        "chigiri mock reference [TOPIC] [NAME]{} [OPTIONS]",
        input_option(input)
    )
}

pub(super) fn input_option(input: InputKind) -> &'static str {
    match input {
        InputKind::Common => "",
        InputKind::Schema => " --schema SDL",
        InputKind::Project => " --config-dir PROJECT",
        InputKind::Bundle => " --config BUNDLE",
    }
}
