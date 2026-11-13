//! Evaluate editable project settings into configuration that contains no Lua handles.

mod active;
#[cfg(test)]
mod active_tests;
mod agent_guidance;
#[cfg(test)]
mod authoring_boundaries_tests;
#[cfg(test)]
mod budget_tests;
mod capacity;
#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod codegen_tests;
mod comparison;
mod config;
mod config_decode;
mod config_events;
#[cfg(test)]
mod config_events_tests;
#[cfg(test)]
mod config_hooks_tests;
mod config_stamp;
mod config_state;
#[cfg(test)]
mod config_tests;
mod consumer_inventory;
#[cfg(test)]
mod consumer_tests;
mod digest;
mod export;
#[cfg(test)]
mod export_tests;
mod filesystem;
#[cfg(test)]
mod format_tests;
mod generate;
#[cfg(test)]
mod generate_tests;
#[cfg(test)]
mod hooks_management_tests;
#[cfg(test)]
mod hooks_readiness_tests;
mod init;
#[cfg(test)]
mod init_tests;
mod input;
mod inventory;
mod layout;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod legacy_rejection_tests;
mod management;
pub(crate) mod metadata;
mod normalize;
#[cfg(test)]
mod normalize_tests;
mod partial;
#[cfg(test)]
mod partial_tests;
#[cfg(test)]
mod portable_fixture_tests;
mod read;
#[cfg(test)]
mod read_tests;
mod schema_sync;
#[cfg(test)]
mod schema_sync_tests;
#[cfg(test)]
mod simplified_tests;
mod sources;
#[cfg(test)]
mod sources_tests;
mod staging;
mod state;
mod storage;
#[cfg(test)]
mod subscription_tests;
mod summary;
#[cfg(test)]
pub(crate) mod test_support;
mod transaction;
#[cfg(test)]
mod transaction_crash_tests;
#[cfg(test)]
mod transaction_move_tests;
#[cfg(test)]
mod transaction_ownership_tests;
#[cfg(test)]
mod transaction_race_tests;
#[cfg(test)]
mod transaction_recovery_tests;
#[cfg(test)]
mod transaction_test_support;
#[cfg(test)]
mod transaction_tests;
mod update;
mod update_consumers;
mod update_inputs;
mod update_modules;
#[cfg(test)]
mod update_orphan_tests;
mod update_orphans;
mod update_plan;
#[cfg(test)]
mod update_plan_tests;
mod update_report;
#[cfg(test)]
mod update_test_support;
pub(crate) mod validation;

use std::fmt;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

pub use active::{ActiveFile, ActiveKind, ActiveSnapshot};
pub use config::{CONFIG_DECLARATIONS, evaluate_config};
pub use export::export_project;
pub use generate::{GeneratedProject, generate_project};
pub use init::{InitProgress, InitReport, init_project, init_project_with_progress};
pub use input::InputFile;
pub use layout::{Layout, ResolverPath, SeedPath};
pub use partial::Readiness;
pub use read::{read_project, read_project_observed};
pub use schema_sync::{
    SchemaSyncDiagnostic, SchemaSyncReport, SchemaSyncStatus, compare_public_schema,
};
pub use sources::{ProjectSchema, read_project_schema};
pub use update::{codegen_project, update_project};
pub use update_report::{SchemaChange, UpdateAction, UpdateEvent, UpdateReport};

/// Closed project format; its version determines the host API contract.
pub const FORMAT: &str = "chigiri-mock-project/v10";

fn logical(path: &str) -> Result<PortableRelativePath, ProjectError> {
    PortableRelativePath::new(path)
        .map_err(|_| ProjectError::new("ownership", "invalid portable project path"))
}

fn fixed_path(path: &str) -> PortableRelativePath {
    PortableRelativePath::new(path).expect("fixed project path is portable")
}

/// Evaluated configuration that can be bundled without rerunning configuration Luau.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSettings {
    /// Loopback-only listener; port zero is resolved when the server binds.
    pub listen: SocketAddr,
    /// Exact credential-free origins allowed by CORS.
    pub allowed_origins: Vec<String>,
}

impl ServerSettings {
    /// Apply the same listener and CORS policy to every project input format.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if !self.listen.ip().is_loopback()
            || !self.allowed_origins.iter().all(|value| valid_origin(value))
        {
            return Err(ProjectError::new(
                "config",
                "numeric loopback and exact HTTP origins required",
            ));
        }
        Ok(())
    }
}

fn valid_origin(value: &str) -> bool {
    use axum::http::{HeaderValue, Uri};
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https"))
        && uri
            .authority()
            .is_some_and(|authority| !authority.as_str().contains(['@', '*']))
        && uri
            .path_and_query()
            .is_none_or(|path| path.as_str().is_empty() || path.as_str() == "/")
        && !value.ends_with('/')
        && !value.contains(['?', '#'])
        && HeaderValue::from_str(value).is_ok()
}

/// Evaluated project settings retained after the configuration VM is destroyed.
pub struct ProjectSettings {
    /// Explicit nonempty ordered SDL source paths.
    pub sources: Vec<String>,
    /// Explicit storage-only SDL references; an empty list is valid.
    pub storage_sources: Vec<String>,
    /// Application time selected independently from execution deadlines.
    pub clock: crate::clock::ClockSettings,
    /// Validated server settings.
    pub server: ServerSettings,
    /// Explicit registrations for the six fixed hook slots.
    pub hooks: crate::hooks::HookSettings,
    /// Validated state declaration and initial value, with no type inference from values.
    pub state: crate::hooks::StateContract,
    /// Validated source-stage limits and explicit named consumer registrations.
    pub events: crate::events::EventSettings,
}

/// Pre-boot diagnostic that omits input source, setting values, and physical paths.
#[derive(Clone, Debug)]
pub struct ProjectError {
    /// CLI-selected logical file name; no physical path is retained.
    pub file: PortableRelativePath,
    /// SDL-derived coordinate or `model.field`; user record values are excluded.
    pub coordinate: std::sync::Arc<str>,
    /// One-based line and column in the logical source.
    pub position: (usize, usize),
    /// Exit code: one for project errors, two for environment or I/O failures.
    pub exit: u8,
    /// Recovery and cleanup failures that accompany, but do not replace, the primary error.
    pub recovery: Vec<ProjectError>,
    /// Whether publication committed before a later verification or cleanup failure.
    pub committed: bool,
    file_known: bool,
    coordinate_known: bool,
    position_known: bool,
    hook_slot: Option<crate::hooks::HookSlot>,
    details: Box<crate::luau::DiagnosticDetails>,
}

impl ProjectError {
    pub(crate) fn new(phase: &'static str, message: &'static str) -> Self {
        let (cause, expected, actual, hint) = crate::luau::diagnostic_shape(phase);
        Self {
            file: fixed_path("mock.luau"),
            coordinate: "config".into(),
            position: (1, 1),
            exit: if phase == "environment" { 2 } else { 1 },
            recovery: Vec::new(),
            committed: false,
            file_known: false,
            coordinate_known: false,
            position_known: false,
            hook_slot: None,
            details: Box::new(crate::luau::DiagnosticDetails {
                consumer: None,
                phase,
                cause,
                member: None,
                expected: expected.into(),
                actual: actual.into(),
                hint: hint.into(),
                message,
            }),
        }
    }

    /// Whether any logical source identity or position was established.
    pub fn location_known(&self) -> bool {
        self.file_known || self.coordinate_known || self.position_known || self.member.is_some()
    }

    /// Whether the logical source file was established.
    pub fn file_known(&self) -> bool {
        self.file_known
    }

    /// Whether a GraphQL coordinate was established.
    pub fn coordinate_known(&self) -> bool {
        self.coordinate_known
    }

    /// Whether the one-based source position was established.
    pub fn position_known(&self) -> bool {
        self.position_known
    }

    /// Canonical hook identity retained separately from GraphQL coordinates.
    pub fn hook_slot(&self) -> Option<crate::hooks::HookSlot> {
        self.hook_slot
    }

    pub(crate) fn set_file(&mut self, file: impl ToString) {
        self.file =
            PortableRelativePath::new(&file.to_string()).unwrap_or_else(|_| fixed_path("<path>"));
        self.file_known = true;
    }

    pub(crate) fn set_coordinate(&mut self, coordinate: impl Into<std::sync::Arc<str>>) {
        self.coordinate = coordinate.into();
        self.coordinate_known = true;
    }

    pub(crate) fn set_position(&mut self, position: (usize, usize)) {
        self.position = position;
        self.position_known = true;
    }

    pub(crate) fn set_hook_slot(&mut self, slot: crate::hooks::HookSlot) {
        self.hook_slot = Some(slot);
        if self.member.is_none() {
            self.member = Some(slot.name().into());
        }
    }
}

impl Deref for ProjectError {
    type Target = crate::luau::DiagnosticDetails;

    fn deref(&self) -> &Self::Target {
        &self.details
    }
}

impl DerefMut for ProjectError {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.details
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.file_known {
            write!(f, "{}", self.file)?;
            if self.position_known {
                write!(f, ":{}:{}", self.position.0, self.position.1)?;
            }
            if self.coordinate_known {
                write!(f, " {}", self.coordinate)?;
            } else if let Some(member) = &self.member {
                write!(f, " {member}")?;
            }
            write!(f, " ")?;
        } else if self.coordinate_known {
            write!(f, "{} ", self.coordinate)?;
        } else if let Some(member) = &self.member {
            write!(f, "{member} ")?;
        }
        write!(f, "[{}] {}", self.phase, self.message)?;
        write!(
            f,
            " ({}; {} != {}; {})",
            self.cause, self.expected, self.actual, self.hint
        )?;
        if self.committed {
            write!(f, " (placement committed)")?;
        }
        for failure in &self.recovery {
            write!(f, "\nrecovery: {failure}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ProjectError {}
