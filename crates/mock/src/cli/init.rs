//! Report project creation separately from subsequent readiness checks.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use chigiri_mock::project::InitProgress;

use crate::diagnostic::Diagnostic;

pub(super) fn run(paths: Vec<PathBuf>, output: &Path) -> Result<(), Diagnostic> {
    let terminal = std::io::stderr().is_terminal();
    let start = Instant::now();
    let report = chigiri_mock::project::init_project_with_progress(&paths, output, move |stage| {
        if terminal {
            let label = match stage {
                InitProgress::ReadingSchema => "reading schema",
                InitProgress::GeneratingTypes => "generating types",
                InitProgress::ValidatingModules => "checking types and modules",
                InitProgress::WritingFiles => "writing project",
            };
            // Progress-display failure does not change the placement result.
            let _ = writeln!(
                std::io::stderr().lock(),
                "chigiri mock init [{:.1}s]: {label}",
                start.elapsed().as_secs_f64()
            );
        }
    })
    .map_err(Diagnostic::project)?;
    super::schema_warnings::emit_cycles(&report.required_reference_cycles);
    println!("chigiri mock init: project created; validation_checked = false");
    Ok(())
}
