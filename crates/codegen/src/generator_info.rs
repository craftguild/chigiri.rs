//! Converts standard-library-only core metadata to closed tool JSON.

#[cfg(test)]
mod tests;

use chigiri_core::{GeneratorBuild, GeneratorInfo};
use serde::{Deserialize, Serialize};

use crate::CodegenError;

const MAX_INFO_BYTES: usize = 4096;

include!(concat!(env!("OUT_DIR"), "/generator-info.rs"));

/// Returns metadata captured when this codegen library or CLI was built.
pub fn generator_info() -> &'static GeneratorInfo {
    &GENERATOR_INFO
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InfoRecord {
    schema_version: String,
    version: String,
    revision: String,
    #[serde(deserialize_with = "required_option")]
    git_commit: Option<String>,
    #[serde(deserialize_with = "required_option")]
    dirty: Option<bool>,
    build: BuildRecord,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildRecord {
    rustc: String,
    host: String,
    target: String,
    profile: String,
}

/// It reads generators from the JSON that is closed in all layers, and it also refuses to miss the nullable key.
pub fn read_generator_info(bytes: &[u8]) -> Result<GeneratorInfo, CodegenError> {
    if bytes.len() > MAX_INFO_BYTES {
        return Err(invalid());
    }
    let record: InfoRecord = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    record.try_into()
}

/// Serialize verified generator information as a JSON document.
pub fn write_generator_info(info: &GeneratorInfo) -> Result<String, CodegenError> {
    info.validate().map_err(|_| invalid())?;
    let mut json = serde_json::to_string_pretty(&InfoRecord::from(info)).map_err(|_| invalid())?;
    json.push('\n');
    if json.len() > MAX_INFO_BYTES {
        return Err(invalid());
    }
    Ok(json)
}

impl From<&GeneratorInfo> for InfoRecord {
    fn from(info: &GeneratorInfo) -> Self {
        Self {
            schema_version: info.schema_version.to_string(),
            version: info.version.to_string(),
            revision: info.revision.to_string(),
            git_commit: info.git_commit.as_ref().map(ToString::to_string),
            dirty: info.dirty,
            build: BuildRecord {
                rustc: info.build.rustc.to_string(),
                host: info.build.host.to_string(),
                target: info.build.target.to_string(),
                profile: info.build.profile.to_string(),
            },
        }
    }
}

impl TryFrom<InfoRecord> for GeneratorInfo {
    type Error = CodegenError;

    fn try_from(record: InfoRecord) -> Result<Self, Self::Error> {
        let info = Self {
            schema_version: record.schema_version.into(),
            version: record.version.into(),
            revision: record.revision.into(),
            git_commit: record.git_commit.map(Into::into),
            dirty: record.dirty,
            build: GeneratorBuild {
                rustc: record.build.rustc.into(),
                host: record.build.host.into(),
                target: record.build.target.into(),
                profile: record.build.profile.into(),
            },
        };
        info.validate().map_err(|_| invalid())?;
        Ok(info)
    }
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

fn invalid() -> CodegenError {
    CodegenError::new(
        "SF4021",
        "Generator information violates its schema, limits, or consistency requirements.",
    )
}
