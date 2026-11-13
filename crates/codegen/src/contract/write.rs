//! Common transaction input for newly created and owned output updates.

#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod input_tests;
mod private_compile;
mod recovery;
#[cfg(test)]
mod term_tests;
#[cfg(test)]
mod tests;
mod transaction;

use crate::{CodegenError, DiagnosticPolicy, DiagnosticReport, ResolvedManifest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Generate,
    Update,
}

/// Generate a new live output only after its candidate passes validation.
pub fn generate_contract(
    manifest: &ResolvedManifest,
    policy: DiagnosticPolicy,
) -> Result<DiagnosticReport, CodegenError> {
    write(manifest, Operation::Generate, policy)
}

/// Replace an owned existing crate only after its candidate passes validation.
pub fn update_contract(
    manifest: &ResolvedManifest,
    policy: DiagnosticPolicy,
) -> Result<DiagnosticReport, CodegenError> {
    write(manifest, Operation::Update, policy)
}

fn write(
    manifest: &ResolvedManifest,
    operation: Operation,
    policy: DiagnosticPolicy,
) -> Result<DiagnosticReport, CodegenError> {
    private_compile::preflight(manifest)?;
    let mut transaction = transaction::Transaction::begin(manifest, operation)?;
    let prepared = (|| {
        let rendered = super::render_contract(manifest)?;
        transaction.prepare(rendered)?;
        let report = policy.enforce(private_compile::check(manifest, &transaction)?)?;
        manifest.verify_paths()?;
        transaction.install(manifest, &mut |_| Ok(()))?;
        Ok(report)
    })();
    match prepared {
        Ok(report) => Ok(report),
        Err(error) => Err(transaction.finish_failure(error)),
    }
}
