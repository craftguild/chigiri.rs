//! Records every generated file except itself, plus generator, input, and runtime provenance.

mod inputs;
mod inventory;
mod read;
pub(super) use inputs::verify as verify_inputs;
pub(crate) use read::read;

use std::collections::BTreeMap;

use chigiri_core::GeneratorInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::config::ResolvedBindings;
use crate::discovery::DiscoveredSource;
use crate::filesystem::PortableRelativePath;
use crate::generator_info::InfoRecord;
use crate::provenance::{lowercase_hex, schema_content_digest};
use crate::{CodegenError, ResolvedManifest, RuntimeSource};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Marker {
    pub(super) schema_version: String,
    pub(super) package: PackageRecord,
    pub(super) abi: u32,
    pub(super) generator: InfoRecord,
    pub(super) runtime: RuntimeRecord,
    pub(super) provenance: Provenance,
    pub(super) files: Vec<FileRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PackageRecord {
    name: String,
    version: String,
    edition: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum RuntimeRecord {
    Git { url: String, revision: String },
    Path { root: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Provenance {
    manifest_label: String,
    manifest_sha256: String,
    schema_sha256: String,
    binding_sha256: String,
    sources: Vec<FileRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileRecord {
    path: PortableRelativePath,
    bytes: u64,
    sha256: String,
}

pub(super) fn render(
    manifest: &ResolvedManifest,
    sources: &[DiscoveredSource],
    bindings: &ResolvedBindings,
    info: &GeneratorInfo,
    files: &BTreeMap<PortableRelativePath, Vec<u8>>,
) -> Result<String, CodegenError> {
    let config = manifest.manifest();
    let package = config.package();
    let marker = Marker {
        schema_version: super::GENERATED_MARKER_SCHEMA.to_owned(),
        package: PackageRecord {
            name: package.name.clone(),
            version: package.version.clone(),
            edition: package.edition.clone(),
        },
        abi: 8,
        generator: InfoRecord::from(info),
        runtime: match config.runtime() {
            RuntimeSource::Git { url, revision, .. } => RuntimeRecord::Git {
                url: url.clone(),
                revision: revision.clone(),
            },
            RuntimeSource::Path { root } => RuntimeRecord::Path { root: root.clone() },
        },
        provenance: Provenance {
            manifest_label: manifest.manifest_label().to_owned(),
            manifest_sha256: digest(manifest.manifest_bytes()),
            schema_sha256: schema_content_digest(sources).to_hex(),
            binding_sha256: lowercase_hex(&bindings.semantic_digest()),
            sources: sources
                .iter()
                .map(|source| record(&source.relative_path, source.contents.as_bytes()))
                .collect(),
        },
        files: files
            .iter()
            .map(|(path, bytes)| record(path, bytes))
            .collect(),
    };
    let mut json = serde_json::to_string_pretty(&marker).map_err(|_| invalid())?;
    json.push('\n');
    if json.len() > 1024 * 1024 {
        return Err(invalid());
    }
    Ok(json)
}

fn record(path: &PortableRelativePath, bytes: &[u8]) -> FileRecord {
    FileRecord {
        path: path.clone(),
        bytes: bytes.len() as u64,
        sha256: digest(bytes),
    }
}

fn digest(bytes: &[u8]) -> String {
    lowercase_hex(&Sha256::digest(bytes))
}

fn invalid() -> CodegenError {
    CodegenError::new(
        "SF4020",
        "The generated ownership marker exceeds its size limit.",
    )
}
