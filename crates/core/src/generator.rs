//! Build-time generator identity reported by the CLI, independent of runtime
//! crate versions.

mod shape;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt;

/// Schema identifier for generator information read and written by this version.
pub const GENERATOR_INFO_SCHEMA: &str = "chigiri/generator-info-v1";

/// Identity recorded when the CLI was built.
///
/// Different uncommitted content built from the same commit may share the same
/// `hash-dirty` revision. `unknown` records unavailable provenance; it does not
/// claim a clean build or prohibit production. Static literals and owned strings
/// use the same record type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorInfo {
    /// Schema identifier for this structure.
    pub schema_version: Cow<'static, str>,
    /// Generator package semver.
    pub version: Cow<'static, str>,
    /// Full commit with optional `-dirty`, or `unknown`.
    pub revision: Cow<'static, str>,
    /// Full lowercase 40-digit commit when both HEAD and status are observable.
    pub git_commit: Option<Cow<'static, str>>,
    /// Whether staged, unstaged, untracked, or submodule changes were observed.
    pub dirty: Option<bool>,
    /// Compiler and platform information for the CLI build.
    pub build: GeneratorBuild,
}

/// CLI build information without credentials or timestamps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorBuild {
    /// `rustc -V` banner used for the build, or `unknown` when unavailable.
    pub rustc: Cow<'static, str>,
    /// Build host target triple.
    pub host: Cow<'static, str>,
    /// CLI binary target triple.
    pub target: Cow<'static, str>,
    /// Cargo profile name.
    pub profile: Cow<'static, str>,
}

/// The record has an invalid shape, limit, or field combination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratorInfoError;

impl fmt::Display for GeneratorInfoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("generator information is invalid")
    }
}

impl std::error::Error for GeneratorInfoError {}

impl GeneratorInfo {
    /// Check the bounded shape of the entire field and the closed combination of revision/commit/dirty.
    pub fn validate(&self) -> Result<(), GeneratorInfoError> {
        let consistent = match (&self.git_commit, self.dirty) {
            (None, None) => self.revision == "unknown",
            (Some(commit), Some(dirty)) if shape::commit(commit) => {
                if dirty {
                    self.revision.strip_suffix("-dirty") == Some(commit.as_ref())
                } else {
                    self.revision == *commit
                }
            }
            _ => false,
        };
        if self.schema_version != GENERATOR_INFO_SCHEMA
            || !shape::semver(&self.version)
            || !consistent
            || !shape::rustc(&self.build.rustc)
            || !shape::token(&self.build.host)
            || !shape::token(&self.build.target)
            || !shape::token(&self.build.profile)
        {
            return Err(GeneratorInfoError);
        }
        Ok(())
    }
}
