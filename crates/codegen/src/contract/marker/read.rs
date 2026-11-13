//! Validates the closed ownership marker and complete inventory from a captured tree.

use chigiri_core::GeneratorInfo;

use crate::filesystem::{CapturedTree, relative_components};
use crate::{CodegenError, Manifest};

use super::super::{GENERATED_MARKER_FILE, GENERATED_MARKER_SCHEMA};
use super::inventory::validate_inventory;
use super::{FileRecord, Marker, RuntimeRecord};

pub(crate) fn read(tree: &CapturedTree) -> Result<GeneratorInfo, CodegenError> {
    let bytes = tree.files.get(GENERATED_MARKER_FILE).ok_or_else(invalid)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid());
    }
    let marker: Marker = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    // ABI 7 is ownership provenance for update only; rendering and execution use ABI 8.
    if marker.schema_version != GENERATED_MARKER_SCHEMA || !matches!(marker.abi, 7 | 8) {
        return Err(invalid());
    }
    validate_settings(&marker)?;
    validate_sources(&marker)?;
    validate_inventory(&marker, tree)?;
    let info: GeneratorInfo = marker.generator.try_into()?;
    crate::write_generator_info(&info)?;
    let rust = super::super::generator::render(&info)?;
    let recorded = tree
        .files
        .get("src/__chigiri/generator_info.rs")
        .map(Vec::as_slice);
    if recorded != Some(rust.as_bytes()) {
        return Err(CodegenError::new(
            "SF4021",
            "Generated Rust provenance does not match the ownership marker.",
        ));
    }
    Ok(info)
}

fn validate_settings(marker: &Marker) -> Result<(), CodegenError> {
    let quote = |text: &str| toml_edit::Value::from(text).to_string();
    let runtime = match &marker.runtime {
        RuntimeRecord::Path { root } => format!("source=\"path\"\nroot={}\n", quote(root)),
        RuntimeRecord::Git { url, revision } => format!(
            "source=\"git\"\nurl={}\nrevision={}\n",
            quote(url),
            quote(revision)
        ),
    };
    let source = format!(
        "schema_version=\"chigiri/config-v2\"\n[schema]\ndirectory=\"graphql\"\n[generation]\noutput=\"generated/contract\"\n[package]\nname={}\nversion={}\nedition={}\n[runtime]\n{runtime}",
        quote(&marker.package.name),
        quote(&marker.package.version),
        quote(&marker.package.edition),
    );
    Manifest::parse("<generated marker>", &source).map_err(|_| invalid())?;
    Ok(())
}

fn validate_sources(marker: &Marker) -> Result<(), CodegenError> {
    let provenance = &marker.provenance;
    relative_components(&provenance.manifest_label).map_err(|_| invalid())?;
    if ![
        &provenance.manifest_sha256,
        &provenance.schema_sha256,
        &provenance.binding_sha256,
    ]
    .into_iter()
    .all(|value| hex(value))
        || provenance.sources.is_empty()
        || provenance.sources.len() > 1024
    {
        return Err(invalid());
    }
    let mut previous = None;
    let mut total = 0u64;
    for source in &provenance.sources {
        validate_record(source, &mut previous)?;
        if !(source.path.ends_with(".graphql") || source.path.ends_with(".gql"))
            || source.bytes > 256 * 1024
        {
            return Err(invalid());
        }
        total = total.checked_add(source.bytes).ok_or_else(invalid)?;
    }
    if total > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn validate_record<'a>(
    record: &'a FileRecord,
    previous: &mut Option<&'a str>,
) -> Result<(), CodegenError> {
    if previous.is_some_and(|path| path >= record.path.as_str()) || !hex(&record.sha256) {
        return Err(invalid());
    }
    *previous = Some(record.path.as_str());
    Ok(())
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn invalid() -> CodegenError {
    CodegenError::new(
        "SF4020",
        "The generated ownership marker is invalid or exceeds a resource limit.",
    )
}
