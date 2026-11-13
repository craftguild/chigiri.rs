//! Build every byte of a standalone generated crate from one resolved manifest.

mod cargo;
mod check;
mod differences;
mod generator;
mod marker;
mod write;

pub use check::{ContractCheck, check_contract};
pub use write::{generate_contract, update_contract};

use std::collections::BTreeMap;

use chigiri_core::GeneratorInfo;

use crate::config::ResolvedBindings;
use crate::filesystem::PortableRelativePath;
use crate::{CodegenError, ResolvedManifest};

/// Schema identifier for the generated-crate ownership marker.
pub const GENERATED_MARKER_SCHEMA: &str = "chigiri/generated-crate-v1";
/// File name recording generated-output ownership and provenance.
pub const GENERATED_MARKER_FILE: &str = "chigiri-generated.json";

/// Complete canonical generated crate, excluding handwritten files and Cargo caches.
pub struct RenderedContract {
    pub(crate) files: BTreeMap<PortableRelativePath, Vec<u8>>,
}

impl RenderedContract {
    /// Iterate over root-relative generated files and canonical bytes.
    pub fn files(&self) -> impl ExactSizeIterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
    }
}

/// Renders the complete contract using real build provenance.
pub fn render_contract(manifest: &ResolvedManifest) -> Result<RenderedContract, CodegenError> {
    render_recorded(manifest, crate::generator_info())
}

pub(crate) fn render_recorded(
    manifest: &ResolvedManifest,
    info: &GeneratorInfo,
) -> Result<RenderedContract, CodegenError> {
    crate::cancellation::check()?;
    info.validate().map_err(|_| {
        CodegenError::new(
            "SF4021",
            "Generator provenance failed integrity validation.",
        )
    })?;
    let compiled = manifest.compile()?;
    let bindings = ResolvedBindings::from_manifest(manifest, &compiled.schema)?;
    let cargo = cargo::render(manifest)?;
    let mut files = BTreeMap::new();
    for file in crate::render::render_schema(&compiled, &bindings)? {
        let name = if file.name == "mod.rs" {
            portable("src/lib.rs")?
        } else {
            portable(&format!("src/{}", file.name))?
        };
        let mut contents = file.contents;
        if name.as_str() == "src/lib.rs" {
            contents.insert_str(0, "#![forbid(unsafe_code)]\n");
            contents.push_str(generator::ROOT);
            contents = crate::formatter::format_source("contract crate root", &contents)?;
        }
        files.insert(name, contents.into_bytes());
    }
    files.insert(portable("Cargo.toml")?, cargo.into_bytes());
    files.insert(
        portable("src/__chigiri/generator_info.rs")?,
        generator::render(info)?.into_bytes(),
    );
    validate(&files)?;
    let marker = marker::render(manifest, &compiled.sources, &bindings, info, &files)?;
    files.insert(portable(GENERATED_MARKER_FILE)?, marker.into_bytes());
    validate(&files)?;
    manifest.verify_paths()?;
    Ok(RenderedContract { files })
}

fn validate(files: &BTreeMap<PortableRelativePath, Vec<u8>>) -> Result<(), CodegenError> {
    let mut total = 0usize;
    for bytes in files.values() {
        total = total.checked_add(bytes.len()).ok_or_else(limit)?;
    }
    if files.len() > 65_536 || total > 64 * 1024 * 1024 {
        return Err(limit());
    }
    Ok(())
}

fn portable(path: &str) -> Result<PortableRelativePath, CodegenError> {
    PortableRelativePath::new(path).map_err(|_| limit())
}

fn limit() -> CodegenError {
    CodegenError::new(
        "SF4010",
        "The crate generated is beyond the path or resource limit.",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn sg_layout_01_rendered_path_and_rust_over_400_lines_are_allowed() {
        for name in ["../outside.rs", "/outside.rs"] {
            assert_eq!(
                super::portable(name).unwrap_err().report().diagnostics()[0].code(),
                "SF4010"
            );
        }
        let files = BTreeMap::from([(
            super::portable("src/lib.rs").unwrap(),
            b"pub struct Value;\n".repeat(401),
        )]);
        super::validate(&files).unwrap();
    }
}
