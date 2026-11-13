//! Machine-readable and human-readable readiness checks share one validator.

use super::check_navigation::{CheckNavigation, InputMode};
use super::check_report::{CheckReport, ReportDiagnostic};
use crate::{config::Config, diagnostic::Diagnostic};
use chigiri_mock::project::{ActiveSnapshot, Readiness};
use std::{collections::BTreeSet, path::PathBuf};

pub(super) fn run(args: &clap::ArgMatches) -> Result<(), Diagnostic> {
    let json = args.get_one::<String>("format").map(String::as_str) == Some("json");
    let targets = args
        .get_many::<String>("target")
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let mut reported_targets = Vec::new();
    let mut warnings = Vec::new();
    let mut navigation = None;
    let result = if targets.is_empty() {
        full(args, json, &mut warnings, &mut navigation).map(|()| true)
    } else {
        selected(
            args.get_one::<PathBuf>("config-dir")
                .expect("clap requires a project for target checks"),
            &targets,
            &mut reported_targets,
            json,
            &mut warnings,
            &mut navigation,
        )
        .map(|()| false)
    };
    let result = result.map_err(|mut error| {
        if let Some(navigation) = &navigation {
            navigation.attach_text(&mut error);
        }
        error
    });
    if !json {
        return text_result(result, targets.len());
    }
    let scope = if targets.is_empty() {
        "project"
    } else {
        "targets"
    };
    match result {
        Ok(readiness_checked) => {
            let mut report = CheckReport::new(scope, true, readiness_checked);
            report.targets = reported_targets;
            report.diagnostics = warnings;
            write_report(&report)?;
            Ok(())
        }
        Err(mut error) => {
            let mut diagnostic = ReportDiagnostic::from_error(&error);
            if let Some(navigation) = &navigation {
                navigation.attach_report(&mut diagnostic);
            }
            warnings.push(diagnostic);
            let mut report = CheckReport::new(scope, false, false);
            report.targets = reported_targets;
            report.diagnostics = warnings;
            write_report(&report)?;
            error.output_owned = true;
            Err(error)
        }
    }
}

fn full(
    args: &clap::ArgMatches,
    json: bool,
    warnings: &mut Vec<ReportDiagnostic>,
    navigation: &mut Option<CheckNavigation>,
) -> Result<(), Diagnostic> {
    let mode = if args.get_one::<PathBuf>("config").is_some() {
        InputMode::Bundle
    } else {
        InputMode::Project
    };
    let observed = |ir: &chigiri_mock::ir::MockIr| {
        *navigation = CheckNavigation::build(ir, mode);
        observe(ir, json, warnings);
    };
    if let Some(path) = args.get_one::<PathBuf>("config") {
        Config::read_observed(path, observed)?;
    } else {
        Config::project_observed(
            args.get_one::<PathBuf>("config-dir")
                .expect("clap requires one check input"),
            observed,
        )?;
    }
    Ok(())
}

fn selected(
    path: &std::path::Path,
    targets: &[String],
    reported_targets: &mut Vec<String>,
    json: bool,
    warnings: &mut Vec<ReportDiagnostic>,
    navigation: &mut Option<CheckNavigation>,
) -> Result<(), Diagnostic> {
    let snapshot = ActiveSnapshot::capture(path).map_err(Diagnostic::project)?;
    *navigation = CheckNavigation::build(snapshot.schema().ir(), InputMode::Project);
    observe(snapshot.schema().ir(), json, warnings);
    let mut unique = BTreeSet::new();
    if targets.iter().all(|target| {
        unique.insert(target.as_str()) && snapshot.files().iter().any(|file| file.path() == target)
    }) {
        reported_targets.extend(targets.iter().cloned());
    }
    let replacements = targets
        .iter()
        .map(|target| {
            let source = snapshot
                .files()
                .iter()
                .find(|file| file.path() == target)
                .map_or_else(String::new, |file| file.source().to_owned());
            (target.clone(), source)
        })
        .collect::<Vec<_>>();
    let result = snapshot
        .validate_selection(&replacements)
        .map_err(Diagnostic::project)?;
    snapshot.verify_unchanged().map_err(Diagnostic::project)?;
    match result {
        Readiness::Ready | Readiness::NotReady(_) => Ok(()),
        Readiness::NotEvaluated(error) => Err(Diagnostic::project(error)),
    }
}

fn observe(ir: &chigiri_mock::ir::MockIr, json: bool, warnings: &mut Vec<ReportDiagnostic>) {
    let cycles = ir.required_reference_cycles();
    if !json {
        super::schema_warnings::emit_cycles(&cycles);
        return;
    }
    warnings.extend(cycles.iter().map(|cycle| ReportDiagnostic {
        severity: "warning".into(),
        file: None,
        coordinate: None,
        member: Some(super::schema_warnings::bounded_path(cycle.path())),
        line: None,
        column: None,
        embedded: None,
        phase: "schema".into(),
        cause: "MOCK_SCHEMA_CYCLE".into(),
        expected: "finite stored model graph".into(),
        actual: "required reference cycle".into(),
        hint: "Make at least one singular relation in the cycle nullable.".into(),
        message: "required model reference cycle; finite Mock values cannot satisfy this non-null singular cycle".into(),
        references: Vec::new(),
        navigation_hook: None,
        navigation_consumer: None,
    }));
}

fn text_result(result: Result<bool, Diagnostic>, targets: usize) -> Result<(), Diagnostic> {
    let readiness_checked = result?;
    let body = if targets == 0 {
        "chigiri mock check: readiness_checked = true\n".to_owned()
    } else {
        format!(
            "chigiri mock check: targets_valid = {targets}; readiness_checked = {readiness_checked}\n"
        )
    };
    write_body(body.as_bytes())
}

fn write_report(report: &CheckReport) -> Result<(), Diagnostic> {
    let body = super::check_report::encode(report)?;
    write_body(&body)
}

fn write_body(body: &[u8]) -> Result<(), Diagnostic> {
    super::reference_output::direct(&mut std::io::stdout().lock(), body)
}
