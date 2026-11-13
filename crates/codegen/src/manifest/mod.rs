//! Manifest-only generation configuration, with parsing kept separate from
//! filesystem resolution.

#[cfg(test)]
mod config_regression;
mod init;
#[cfg(test)]
mod io_tests;
#[cfg(test)]
mod license_tests;
mod load;
mod parse;
#[cfg(test)]
mod runtime_version_tests;
mod shape;
mod table;
#[cfg(test)]
mod tests;

pub use init::{MANIFEST_TEMPLATE, init_manifest};
pub use load::ResolvedManifest;

use std::collections::{BTreeMap, BTreeSet};

use chigiri_core::schema::{SemanticSchema, TypeKind};

use crate::{CodegenDiagnostic, CodegenError, DiagnosticLabel, DiagnosticReport};

/// The supported generation manifest schema.
pub const MANIFEST_SCHEMA: &str = "chigiri/config-v2";

/// Generation configuration validated at every nesting level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub(crate) schema_directory: String,
    pub(crate) package: Package,
    pub(crate) output: String,
    pub(crate) runtime: RuntimeSource,
    pub(crate) domain_enums: Vec<String>,
    pub(crate) binding_labels: BTreeMap<String, DiagnosticLabel>,
    pub(crate) consumer: Option<Consumer>,
}

/// The declaration of the generated Cargo package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    /// The Cargo package name.
    pub name: String,
    /// Package semver, independent of the generator and runtime versions.
    pub version: String,
    /// Rust edition. The current contract is for 2024.
    pub edition: String,
    /// Optional caller-selected Cargo license expression for the generated package.
    /// Omission does not infer or inherit a license from Chigiri.
    pub license: Option<String>,
}

/// An explicitly selected Chigiri source, with no implicit URL or path fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeSource {
    /// A source pinned to a full commit, normally fetched by Cargo from Git.
    Git {
        /// HTTPS or SSH URL without embedded credentials.
        url: String,
        /// A nonzero, lowercase, 40-character commit hash.
        revision: String,
        /// Optional package SemVer, emitted as an exact Cargo version requirement.
        /// This is independent of the generator and generated package versions.
        version: Option<String>,
    },
    /// A mutable, location-dependent chigiri.rs source root relative to the manifest.
    Path {
        /// Resolve the facade from `crates/facade` under this root.
        root: String,
    },
}

/// The consumer and Cargo configuration checked by `doctor`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Consumer {
    /// Path to `Cargo.toml`, relative to the manifest root.
    pub cargo_manifest: String,
    /// Package to check.
    pub package: String,
    /// Rust dependency alias assigned to the generated crate by the consumer.
    pub generated_dependency: String,
    /// Explicit features. An empty list selects only default features.
    pub features: Vec<String>,
    /// Select all features.
    pub all_features: bool,
    /// Disable default features.
    pub no_default_features: bool,
    /// Check all targets. Defaults to `true`.
    pub all_targets: bool,
}

impl Manifest {
    /// Validate bounded UTF-8 TOML without performing file I/O or invoking Cargo.
    pub fn parse(label: &str, source: &str) -> Result<Self, CodegenError> {
        parse::parse(label, source)
    }

    /// Path to the SDL directory, relative to the manifest root.
    pub fn schema_directory(&self) -> &str {
        &self.schema_directory
    }

    /// Generated Cargo package declaration.
    pub fn package(&self) -> &Package {
        &self.package
    }

    /// Path to the generated crate, relative to the manifest root.
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Explicit runtime source.
    pub fn runtime(&self) -> &RuntimeSource {
        &self.runtime
    }

    /// Opted-in domain enum names.
    pub fn domain_enums(&self) -> &[String] {
        &self.domain_enums
    }

    /// Consumer checked by `doctor`; omitted for generation-only configurations.
    pub fn consumer(&self) -> Option<&Consumer> {
        self.consumer.as_ref()
    }

    /// Match bindings against the SDL and reject types other than source-defined enums.
    pub fn resolve_domain_enums(
        &self,
        schema: &SemanticSchema,
    ) -> Result<BTreeSet<String>, CodegenError> {
        for name in &self.domain_enums {
            if !schema
                .type_by_name(name)
                .is_some_and(|ty| ty.kind() == TypeKind::Enum && ty.name_span().is_some())
            {
                let mut diagnostic = CodegenDiagnostic::error(
                    "SF4012",
                    format!("domain enum `{name}` is not a source-defined enum"),
                );
                if let Some(label) = self.binding_labels.get(name) {
                    diagnostic = diagnostic.with_primary(label.clone());
                }
                return Err(CodegenError::from_report(DiagnosticReport::new(
                    vec![diagnostic],
                    false,
                )));
            }
        }
        Ok(self.domain_enums.iter().cloned().collect())
    }
}
