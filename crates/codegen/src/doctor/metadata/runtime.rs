//! Verifies SDK and Chigiri runtime identity in the selected consumer dependency closure.

#[cfg(test)]
mod tests;

use crate::filesystem::Directory;
use crate::{CodegenError, Consumer, ResolvedManifest, RuntimeSource};

use super::{Metadata, Package, contract, invalid, parent};

const PACKAGES: &[(&str, &str)] = &[
    ("chigiri", "facade"),
    ("chigiri_runtime", "runtime"),
    ("chigiri_core", "core"),
    ("chigiri_operation", "operation"),
    ("chigiri_policy", "policy"),
    ("chigiri_http", "http"),
    ("chigiri_rate", "rate"),
    ("chigiri_rate_memory", "rate-memory"),
    ("chigiri_observability", "observability"),
    ("chigiri_tracing", "tracing"),
    ("chigiri_opentelemetry", "opentelemetry"),
    ("chigiri_axum", "axum"),
    ("chigiri_hyper", "hyper"),
    ("chigiri_testkit", "testkit"),
];

pub(in crate::doctor) fn verify(
    metadata: &Metadata,
    manifest: &ResolvedManifest,
    consumer: &Consumer,
    selected: &Package,
) -> Result<(), CodegenError> {
    let sdk = metadata.direct(
        &selected.id,
        &consumer.generated_dependency,
        consumer.all_targets,
    )?;
    let actual = Directory::open(parent(&sdk.manifest_path)?).map_err(|_| mismatch())?;
    let expected = manifest
        .root
        .relative(manifest.manifest().output())
        .map_err(|_| mismatch())?;
    if sdk.name != manifest.manifest().package().name
        || sdk.version != manifest.manifest().package().version
        || sdk.source.is_some()
        || !actual.same_location(&expected).map_err(|_| mismatch())?
    {
        return Err(contract(
            "The consumer's generated dependency does not match the current generated crate designated by the manifest.",
        ));
    }
    let facade = metadata.direct(&sdk.id, "chigiri", false)?;
    if facade.name != "chigiri" {
        return Err(mismatch());
    }
    let reachable = metadata.reachable(&selected.id, consumer.all_targets)?;
    let mut identities = std::collections::BTreeMap::new();
    for id in reachable {
        let package = metadata.package(&id)?;
        let Some((_, directory)) = PACKAGES.iter().find(|(name, _)| *name == package.name) else {
            continue;
        };
        if identities
            .insert(package.name.as_str(), package.id.as_str())
            .is_some()
        {
            return Err(mismatch());
        }
        match manifest.manifest().runtime() {
            RuntimeSource::Path { root } => {
                if package.source.is_some() {
                    return Err(mismatch());
                }
                let expected = manifest
                    .root
                    .resolve_directory(std::path::Path::new(root))
                    .and_then(|root| root.relative(&format!("crates/{directory}")))
                    .map_err(|_| mismatch())?;
                let actual =
                    Directory::open(parent(&package.manifest_path)?).map_err(|_| mismatch())?;
                if !actual.same_location(&expected).map_err(|_| mismatch())? {
                    return Err(mismatch());
                }
            }
            RuntimeSource::Git { revision, .. } => {
                let source = package.source.as_deref().ok_or_else(mismatch)?;
                if Some(source) != facade.source.as_deref()
                    || !git_source(source, requested_source(sdk)?, revision)?
                {
                    return Err(mismatch());
                }
            }
        }
    }
    if !identities.contains_key("chigiri") {
        return Err(mismatch());
    }
    Ok(())
}

fn requested_source(sdk: &Package) -> Result<&str, CodegenError> {
    // The exact test SDK dependencies are regulated by Cargo itself.
    let mut matching = sdk.dependencies.iter().filter(|dependency| {
        dependency.kind.is_none()
            && dependency.rename.as_deref().unwrap_or(&dependency.name) == "chigiri"
    });
    let requested = matching.next().ok_or_else(invalid)?;
    if matching.next().is_some() || requested.name != "chigiri" {
        return Err(invalid());
    }
    requested.source.as_deref().ok_or_else(invalid)
}

fn git_source(source: &str, requested: &str, revision: &str) -> Result<bool, CodegenError> {
    // Source ID's internal format is detailed in Cargo implementation. It only accepts verified v1 expressions, and does not assume unknown expressions.
    if !source.starts_with("git+") || !requested.starts_with("git+") {
        return Ok(false);
    }
    let Some((resolved_request, commit)) = source.rsplit_once('#') else {
        return Err(invalid());
    };
    let Some((_, requested_rev)) = requested.rsplit_once("?rev=") else {
        return Err(invalid());
    };
    Ok(resolved_request == requested && requested_rev == revision && commit == revision)
}

fn mismatch() -> CodegenError {
    contract(
        "The consumer's Chigiri runtime is mixing different or multiple identities with the explicit source.",
    )
}
