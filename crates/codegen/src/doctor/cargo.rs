//! Runs Cargo checks against the manifest-selected generated product and consumer.

use std::process::Command;
use std::time::Duration;

use crate::{
    CodegenDiagnostic, CodegenError, Consumer, DiagnosticOrigin, ProcessLimits, ProcessOutput,
};

use super::inputs::Inputs;
use super::metadata::Metadata;

pub(super) fn metadata(
    inputs: &Inputs<'_>,
    consumer: &Consumer,
    full: bool,
) -> Result<Metadata, CodegenError> {
    let mut args = vec![
        "metadata".to_owned(),
        "--format-version=1".to_owned(),
        "--locked".to_owned(),
    ];
    if full {
        features(&mut args, consumer);
    } else {
        args.push("--no-deps".to_owned());
    }
    let output = execute(inputs, &args)?;
    if !output.status.success() {
        // Classify only known lock or feature failures; preserve all other stderr.
        let text = String::from_utf8_lossy(&output.stderr);
        if text.contains("lock file") && text.contains("--locked") {
            return Err(super::metadata::contract(
                "The consumer lockfile must be updated.",
            ));
        }
        if text.contains("does not have these features")
            || text.contains("does not have the feature")
            || text.contains("does not have that feature")
            || text.contains("does not contain this feature")
        {
            return Err(super::metadata::contract(
                "A manifest-selected feature is absent from the consumer or dependency package.",
            ));
        }
        return Err(failure());
    }
    Metadata::read(&output.stdout)
}

pub(super) fn check(
    inputs: &Inputs<'_>,
    consumer: &Consumer,
) -> Result<ProcessOutput, CodegenError> {
    execute(inputs, &check_arguments(consumer))
}

fn execute(inputs: &Inputs<'_>, arguments: &[String]) -> Result<ProcessOutput, CodegenError> {
    inputs.verify()?;
    let mut command = Command::new("cargo");
    inputs
        .requested
        .directory
        .set_current_dir(&mut command)
        .map_err(super::inputs::io)?;
    command.args(arguments).arg("--manifest-path");
    inputs
        .selected
        .as_ref()
        .unwrap_or(&inputs.requested)
        .push_argument(&mut command)?;
    let result = crate::run_bounded_process(
        &mut command,
        &[],
        ProcessLimits {
            timeout: Duration::from_secs(300),
            bytes: 64 * 1024 * 1024,
        },
    )
    .map_err(|error| {
        if error == crate::ProcessFailure::Cancelled {
            crate::cancellation::error()
        } else {
            failure()
        }
    });
    let verified = crate::cancellation::recovering(|| inputs.verify());
    match (result, verified) {
        (Ok(output), Ok(())) => Ok(output),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(primary), Err(secondary)) => Err(super::combine(primary, secondary)),
    }
}

fn check_arguments(consumer: &Consumer) -> Vec<String> {
    let mut args = vec![
        "check".to_owned(),
        "--locked".to_owned(),
        "--package".to_owned(),
        consumer.package.clone(),
        "--message-format=json".to_owned(),
    ];
    features(&mut args, consumer);
    if consumer.all_targets {
        args.push("--all-targets".to_owned());
    }
    args
}

fn features(args: &mut Vec<String>, consumer: &Consumer) {
    if consumer.all_features {
        args.push("--all-features".to_owned());
    }
    if consumer.no_default_features {
        args.push("--no-default-features".to_owned());
    }
    if !consumer.features.is_empty() {
        args.extend(["--features".to_owned(), consumer.features.join(",")]);
    }
}

pub(super) fn invocation(inputs: &Inputs<'_>, consumer: &Consumer) -> CodegenDiagnostic {
    CodegenDiagnostic::note("SF4017", "Ran a real consumer cargo check.")
        .with_origin(DiagnosticOrigin::Cargo)
        .with_note(format!(
            "argv: cargo {} --manifest-path {}",
            check_arguments(consumer).join(" "),
            inputs
                .manifest
                .root
                .descendant_label(
                    &inputs
                        .selected
                        .as_ref()
                        .unwrap_or(&inputs.requested)
                        .directory,
                    &inputs.selected.as_ref().unwrap_or(&inputs.requested).name,
                )
                .map_or_else(|_| "<path>".to_owned(), |path| path.to_string())
        ))
}

fn failure() -> CodegenError {
    CodegenError::from_report(crate::DiagnosticReport::new(
        vec![
            CodegenDiagnostic::error("SF4017", "Cargo execution failed.")
                .with_origin(DiagnosticOrigin::Cargo),
        ],
        false,
    ))
}
