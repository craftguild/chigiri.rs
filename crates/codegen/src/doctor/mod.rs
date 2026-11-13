//! Runs an exact static check, then validates the manifest-selected consumer with Cargo.

mod cargo;
mod correlation;
mod inputs;
mod metadata;
mod source;

use crate::{CodegenError, DiagnosticPolicy, DiagnosticReport, ResolvedManifest};

/// Validate generated output, consumer dependencies, and type integrity without updating the lockfile.
pub fn doctor_contract(
    manifest: &ResolvedManifest,
    policy: DiagnosticPolicy,
) -> Result<DiagnosticReport, CodegenError> {
    run(manifest, policy)
}

fn run(
    manifest: &ResolvedManifest,
    policy: DiagnosticPolicy,
) -> Result<DiagnosticReport, CodegenError> {
    let checked = crate::check_contract(manifest).map_err(static_failure)?;
    let schema = crate::inspect_contract_schema(manifest)?;
    let mut notices = Vec::new();
    if checked.generator_differs() {
        notices.push(
            crate::CodegenDiagnostic::note(
                "SF4027",
                "The static contract matches even though the recorded generator differs.",
            )
            .with_note(format!(
                "recorded: {}",
                crate::write_generator_info(&checked.recorded)?
            ))
            .with_note(format!(
                "current: {}",
                crate::write_generator_info(&checked.current)?
            )),
        );
    }
    let mut inputs = inputs::Inputs::new(manifest, checked)?;
    let consumer = manifest
        .manifest()
        .consumer()
        .ok_or_else(|| metadata::contract("Doctor requires consumer settings."))?;
    let result = (|| {
        let preliminary = cargo::metadata(&inputs, consumer, false)?;
        let selected = preliminary.selected(consumer)?;
        validate_features(consumer, selected)?;
        inputs.select(&preliminary, selected)?;
        let graph = cargo::metadata(&inputs, consumer, true)?;
        let selected = graph.selected(consumer)?;
        let workspace =
            crate::filesystem::Directory::open(&graph.workspace_root).map_err(inputs::io)?;
        if graph
            .resolve
            .as_ref()
            .and_then(|resolve| resolve.root.as_deref())
            != Some(selected.id.as_str())
            || !workspace
                .same_location(&inputs.workspace_manifest.as_ref().unwrap().directory)
                .map_err(inputs::io)?
        {
            return Err(metadata::contract(
                "Cargo metadata resolved inconsistent workspace and package roots.",
            ));
        }
        let directory =
            crate::filesystem::Directory::open(metadata::parent(&selected.manifest_path)?)
                .map_err(inputs::io)?;
        if !directory
            .same_location(&inputs.selected.as_ref().unwrap().directory)
            .map_err(inputs::io)?
        {
            return Err(metadata::contract(
                "Cargo metadata resolved inconsistent consumer package locations.",
            ));
        }
        inputs.cache(&graph, selected)?;
        metadata::runtime::verify(&graph, manifest, consumer, selected)?;
        inputs.verify()?;
        let sources = source::Sources::new(
            manifest.root.clone(),
            workspace,
            manifest
                .root
                .relative(manifest.manifest().output())
                .map_err(inputs::io)?,
            &consumer.generated_dependency,
        );
        let output = cargo::check(&inputs, consumer)?;
        notices.push(cargo::invocation(&inputs, consumer));
        let report = crate::cargo_diagnostics::compile_with(
            &output,
            &|path| sources.label(path),
            &|record| {
                if record.get("package_id").and_then(serde_json::Value::as_str)
                    == Some(selected.id.as_str())
                {
                    correlation::supplements(record, &schema, &sources)
                } else {
                    Vec::new()
                }
            },
        )?;
        inputs.verify()?;
        policy.enforce(report)
    })();
    match result {
        Ok(report) => {
            notices.extend(report.diagnostics().iter().cloned());
            policy.enforce(DiagnosticReport::new(notices, report.truncated()))
        }
        Err(error) => {
            notices.extend(error.report().diagnostics().iter().cloned());
            Err(CodegenError::from_report(DiagnosticReport::new(
                notices,
                error.report().truncated(),
            )))
        }
    }
}

fn static_failure(error: CodegenError) -> CodegenError {
    let invalid_record = error
        .report()
        .diagnostics()
        .iter()
        .any(|diagnostic| matches!(diagnostic.code(), "SF4020" | "SF4021"));
    let drift = error
        .report()
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code() == "SF4009");
    if !invalid_record && !drift {
        return error;
    }
    let fallback_help = if invalid_record {
        "Save the existing tree with a different name and run chigiri rust codegen without any source."
    } else {
        "Please update the product with chigiri rust update."
    };
    let help = error
        .report()
        .diagnostics()
        .iter()
        .flat_map(crate::CodegenDiagnostic::help)
        .next()
        .map(String::as_str)
        .unwrap_or(fallback_help);
    combine(
        CodegenError::from_report(DiagnosticReport::new(
            vec![
                crate::CodegenDiagnostic::error(
                    "SF4016",
                    "The generated crate failed its static coherence check.",
                )
                .with_help(help),
            ],
            false,
        )),
        error,
    )
}

fn validate_features(
    consumer: &crate::Consumer,
    package: &metadata::Package,
) -> Result<(), CodegenError> {
    if consumer
        .features
        .iter()
        .any(|feature| !feature.contains('/') && !package.features.contains_key(feature))
    {
        return Err(metadata::contract(
            "The consumer does not define a feature selected by the manifest.",
        ));
    }
    Ok(())
}

fn combine(primary: CodegenError, secondary: CodegenError) -> CodegenError {
    let mut diagnostics = primary.report().diagnostics().to_vec();
    diagnostics.extend(secondary.report().diagnostics().iter().cloned());
    CodegenError::from_report(DiagnosticReport::new(
        diagnostics,
        primary.report().truncated() || secondary.report().truncated(),
    ))
}
