//! Compare configured public SDL with the adopted snapshot without checking readiness.

use std::path::Path;

use serde::Serialize;

use super::ProjectError;
use super::filesystem::Directory;
use super::management::Management;
use super::normalize::AdoptedSchema;

const FORMAT: &str = "chigiri-mock-schema-sync/v1";

/// Public freshness result; it does not imply local generation or runtime readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaSyncStatus {
    InSync,
    UpdateRequired,
    Unavailable,
}

/// Sanitized comparison failure without paths or SDL values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaSyncDiagnostic {
    pub phase: &'static str,
    pub message: &'static str,
}

/// Closed machine-readable public-schema comparison report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaSyncReport {
    pub format: &'static str,
    pub status: SchemaSyncStatus,
    pub readiness_checked: bool,
    pub adopted_digest: Option<String>,
    pub input_digest: Option<String>,
    pub diagnostics: Vec<SchemaSyncDiagnostic>,
}

impl SchemaSyncReport {
    pub fn exit_code(&self) -> u8 {
        match self.status {
            SchemaSyncStatus::InSync => 0,
            SchemaSyncStatus::UpdateRequired => 1,
            SchemaSyncStatus::Unavailable => 2,
        }
    }
}

/// Compare only public acquisition inputs with verified adopted ownership.
pub fn compare_public_schema(path: &Path) -> SchemaSyncReport {
    match compare(path) {
        Ok(report) => report,
        Err((adopted, error)) => unavailable(adopted, error),
    }
}

fn compare(path: &Path) -> Result<SchemaSyncReport, (Option<String>, ProjectError)> {
    let directory = Directory::open(path).map_err(|error| (None, error))?;
    directory
        .transaction_absent()
        .map_err(|error| (None, error))?;
    let management = Management::read(&directory).map_err(|error| (None, error))?;
    management.verify().map_err(|error| (None, error))?;
    let adopted = management.snapshot.public_schema.digest.as_str().to_owned();
    let (settings, config) =
        super::sources::read_config(&directory).map_err(|error| (Some(adopted.clone()), error))?;
    let (public, files) = super::sources::read_sources(&directory, &settings.sources)
        .map_err(|error| (Some(adopted.clone()), error))?;
    crate::schema::parse_sources(public.clone()).map_err(|error| {
        (
            Some(adopted.clone()),
            ProjectError::new("schema", error.code),
        )
    })?;
    let input = AdoptedSchema::from_sources(&public)
        .map_err(|error| (Some(adopted.clone()), error))?
        .digest
        .as_str()
        .to_owned();
    directory
        .verify()
        .and_then(|()| config.file.verify())
        .and_then(|()| files.iter().try_for_each(|input| input.file.verify()))
        .and_then(|()| management.verify())
        .and_then(|()| directory.transaction_absent())
        .map_err(|error| (Some(adopted.clone()), error))?;
    let status = if adopted == input {
        SchemaSyncStatus::InSync
    } else {
        SchemaSyncStatus::UpdateRequired
    };
    Ok(SchemaSyncReport {
        format: FORMAT,
        status,
        readiness_checked: false,
        adopted_digest: Some(adopted),
        input_digest: Some(input),
        diagnostics: Vec::new(),
    })
}

fn unavailable(adopted: Option<String>, error: ProjectError) -> SchemaSyncReport {
    SchemaSyncReport {
        format: FORMAT,
        status: SchemaSyncStatus::Unavailable,
        readiness_checked: false,
        adopted_digest: adopted,
        input_digest: None,
        diagnostics: vec![SchemaSyncDiagnostic {
            phase: error.phase,
            message: error.message,
        }],
    }
}
