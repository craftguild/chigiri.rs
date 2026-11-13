//! Report public freshness separately from adoption and local generation.

use std::path::Path;

use chigiri_mock::project::{SchemaSyncReport, SchemaSyncStatus, UpdateReport};

use crate::diagnostic::Diagnostic;

pub(super) fn run(args: &clap::ArgMatches, path: &Path) -> Result<(), Diagnostic> {
    if args.get_flag("check") {
        compare(path, args.get_one::<String>("format").map(String::as_str))
    } else {
        let report = chigiri_mock::project::update_project(path).map_err(Diagnostic::project)?;
        placement("update", report)
    }
}

pub(super) fn codegen(path: &Path) -> Result<(), Diagnostic> {
    let report = chigiri_mock::project::codegen_project(path).map_err(Diagnostic::project)?;
    placement("codegen", report)
}

fn compare(path: &Path, format: Option<&str>) -> Result<(), Diagnostic> {
    let report = chigiri_mock::project::compare_public_schema(path);
    let exit = report.exit_code();
    let body = if format == Some("json") {
        let mut body = serde_json::to_vec_pretty(&report)
            .map_err(|_| Diagnostic::environment("cannot render freshness report"))?;
        body.push(b'\n');
        body
    } else {
        text(&report).into_bytes()
    };
    super::reference_output::direct(&mut std::io::stdout().lock(), &body)?;
    if exit == 0 {
        Ok(())
    } else {
        Err(Diagnostic::reported_status(exit))
    }
}

fn text(report: &SchemaSyncReport) -> String {
    match report.status {
        SchemaSyncStatus::InSync => concat!(
            "chigiri mock update --check: in_sync; public SDL matches the adopted contract; ",
            "readiness_checked = false\n"
        )
        .into(),
        SchemaSyncStatus::UpdateRequired => concat!(
            "chigiri mock update --check: update_required; run `chigiri mock update` to adopt ",
            "the current public SDL; readiness_checked = false\n"
        )
        .into(),
        SchemaSyncStatus::Unavailable => {
            let diagnostic = report.diagnostics.first();
            format!(
                "chigiri mock update --check: unavailable; {}: {}; public freshness is unknown; readiness_checked = false\n",
                diagnostic.map_or("read", |item| item.phase),
                diagnostic.map_or("public SDL is unavailable", |item| item.message),
            )
        }
    }
}

fn placement(command: &str, report: UpdateReport) -> Result<(), Diagnostic> {
    let mut body = String::new();
    for change in report.schema_changes {
        body.push_str(&format!("schema {}: {}\n", change.action, change.element));
    }
    for event in report.events {
        body.push_str(&format!("{}: {}\n", event.action.name(), event.path));
    }
    body.push_str(&format!(
        "chigiri mock {command}: placement complete; schema_changed = {}; validation_checked = false\n",
        report.schema_changed
    ));
    body.push_str("Review proposals and preserved modules/seeds, then run check before serving.\n");
    super::reference_output::direct(&mut std::io::stdout().lock(), body.as_bytes())
}
