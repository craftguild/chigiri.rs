//! Rust command adapter, diagnostics, and exit classification.

use std::ffi::OsString;
use std::io::Write as _;

use chigiri_codegen::{
    CodegenDiagnostic, CodegenError, DiagnosticFormat, DiagnosticOrigin, DiagnosticPolicy,
    DiagnosticReport, DiagnosticSeverity, ResolvedManifest,
};

use crate::grammar::{ContractCommand, RustAction};

pub fn run(action: RustAction) -> u8 {
    match chigiri_codegen::with_signal_cancellation(|| execute(action)) {
        Ok(status) => status,
        Err(error) => {
            write_error(&error, DiagnosticFormat::Human);
            2
        }
    }
}

fn execute(action: RustAction) -> u8 {
    match action {
        RustAction::Init { manifest, format } => match chigiri_codegen::init_manifest(&manifest) {
            Ok(()) => 0,
            Err(error) => {
                let input = error
                    .report()
                    .diagnostics()
                    .iter()
                    .any(|item| item.code() == "SF4011");
                write_error(&error, format);
                if input { 1 } else { 2 }
            }
        },
        RustAction::Contract {
            command,
            manifest,
            format,
            deny_warnings,
        } => contract(command, &manifest, format, deny_warnings),
    }
}

fn contract(
    command: ContractCommand,
    path: &std::path::Path,
    format: DiagnosticFormat,
    deny_warnings: bool,
) -> u8 {
    let result = ResolvedManifest::load(path).and_then(|manifest| {
        let policy = DiagnosticPolicy { deny_warnings };
        let mut diagnostics = Vec::new();
        match command {
            ContractCommand::Codegen => return chigiri_codegen::generate_contract(&manifest, policy),
            ContractCommand::Update => return chigiri_codegen::update_contract(&manifest, policy),
            ContractCommand::Doctor => return chigiri_codegen::doctor_contract(&manifest, policy),
            ContractCommand::Check => {
                let checked = chigiri_codegen::check_contract(&manifest)?;
                if checked.generator_differs() {
                    diagnostics.push(CodegenDiagnostic::note(
                        "SF4027",
                        "The recorded generator differs from the currently running generator; \
                         exact comparison succeeded while preserving the recorded generator metadata.",
                    )
                    .with_note(format!(
                        "recorded: {}",
                        chigiri_codegen::write_generator_info(&checked.recorded)?
                    ))
                    .with_note(format!(
                        "current: {}",
                        chigiri_codegen::write_generator_info(&checked.current)?
                    )));
                }
            }
        }
        Ok(DiagnosticReport::new(diagnostics, false))
    });
    let (report, status) = match result {
        Ok(report) => {
            let failed = report.truncated()
                || report.diagnostics().iter().any(|diagnostic| {
                    diagnostic.severity() == DiagnosticSeverity::Error
                        || (deny_warnings && diagnostic.severity() == DiagnosticSeverity::Warning)
                });
            (report, u8::from(failed))
        }
        Err(error) => {
            let status = error_status(&error);
            (error.into_report(), status)
        }
    };
    if (!report.diagnostics().is_empty() || report.truncated())
        && write_report(&report, format).is_err()
    {
        return 2;
    }
    status
}

pub fn version(format: DiagnosticFormat) -> u8 {
    let info = chigiri_codegen::generator_info();
    let output = match format {
        DiagnosticFormat::Json => chigiri_codegen::write_generator_info(info),
        DiagnosticFormat::Human => return crate::version::write("chigiri rust", &[]),
    };
    match output.and_then(write_stdout) {
        Ok(()) => 0,
        Err(error) => {
            write_error(&error, format);
            2
        }
    }
}

pub fn parse_error(message: &str, format: DiagnosticFormat) -> u8 {
    write_error(&CodegenError::new("SF4001", message), format);
    2
}

pub fn diagnostic_format_hint(arguments: &[OsString]) -> Option<DiagnosticFormat> {
    if arguments.first().is_none_or(|value| value != "rust") {
        return None;
    }
    let json = arguments
        .windows(2)
        .any(|pair| pair[0] == "--format" && pair[1] == "json")
        || arguments.iter().any(|arg| arg == "--format=json");
    Some(if json {
        DiagnosticFormat::Json
    } else {
        DiagnosticFormat::Human
    })
}

fn error_status(error: &CodegenError) -> u8 {
    let environment = error.report().diagnostics().iter().any(|diagnostic| {
        diagnostic.severity() == DiagnosticSeverity::Error
            && ((diagnostic.code() == "SF4017" && diagnostic.origin() != DiagnosticOrigin::Rustc)
                || matches!(
                    diagnostic.code(),
                    "SF4001" | "SF4004" | "SF4008" | "SF4022" | "SF4023" | "SF4024" | "SF4026"
                ))
    });
    if environment { 2 } else { 1 }
}

fn write_stdout(output: impl AsRef<[u8]>) -> Result<(), CodegenError> {
    chigiri_codegen::check_cancellation()?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(output.as_ref())
        .and_then(|()| stdout.flush())
        .map_err(|error| CodegenError::new("SF4008", format!("cannot write to stdout: {error}")))
}

fn write_error(error: &CodegenError, format: DiagnosticFormat) {
    let _ = write_report(error.report(), format);
}

fn write_report(report: &DiagnosticReport, format: DiagnosticFormat) -> std::io::Result<()> {
    let rendered = chigiri_codegen::render_diagnostics(report, format);
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(rendered.as_bytes())?;
    stderr.write_all(b"\n")
}
