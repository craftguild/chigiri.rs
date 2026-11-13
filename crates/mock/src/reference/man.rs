//! Fixed manual-style headings shared by plain-text output and the built-in viewer.

use std::fmt;
use std::io::{self, Write};

pub(super) const TITLE: &str = "CHIGIRI-MOCK-REFERENCE(1)";

pub(super) fn title(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "{TITLE}")
}

pub(super) fn identity(
    out: &mut impl Write,
    purpose: &str,
    synopsis: impl fmt::Display,
) -> io::Result<()> {
    section(out, "NAME")?;
    write!(out, "    chigiri-mock-reference - ")?;
    super::text::visible(out, purpose)?;
    writeln!(out)?;
    section(out, "SYNOPSIS")?;
    writeln!(out, "    {synopsis}")
}

pub(super) fn section(out: &mut impl Write, name: &str) -> io::Result<()> {
    writeln!(out, "\n{name}")
}

pub(super) fn see_also(out: &mut impl Write) -> io::Result<()> {
    section(out, "SEE ALSO")?;
    writeln!(out, "    chigiri mock reference")?;
    writeln!(out, "    chigiri mock reference all")
}
