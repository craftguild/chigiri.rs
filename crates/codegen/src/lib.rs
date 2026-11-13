//! Library for generating canonical Rust source from SDL.

#![forbid(unsafe_code)]

mod cancellation;
mod cargo_diagnostics;
mod compile;
mod config;
mod contract;
mod developer;
mod diagnostic_render;
mod discovery;
mod doctor;
mod error;
mod filesystem;
mod formatter;
mod generator_info;
mod literal;
mod manifest;
mod naming;
mod path_redaction;
mod process;
mod provenance;
mod render;
mod writer;

pub use cancellation::Cancellation;
#[doc(hidden)]
pub use cancellation::{check_cancellation, run_bounded_process, with_signal_cancellation};

pub use contract::{
    ContractCheck, GENERATED_MARKER_FILE, GENERATED_MARKER_SCHEMA, RenderedContract,
    check_contract, generate_contract, render_contract, update_contract,
};
#[doc(hidden)]
pub use developer::{DeveloperField, DeveloperObject, DeveloperSchema, inspect_contract_schema};
pub use diagnostic_render::{
    DIAGNOSTIC_REPORT_SCHEMA, DiagnosticFormat, MAX_DIAGNOSTIC_REPORT_BYTES, render_diagnostics,
};
pub use doctor::doctor_contract;
pub use error::{
    CodegenDiagnostic, CodegenError, DiagnosticLabel, DiagnosticOrigin, DiagnosticPolicy,
    DiagnosticReport, DiagnosticSeverity,
};
pub use generator_info::{generator_info, read_generator_info, write_generator_info};
pub use manifest::{
    Consumer, MANIFEST_SCHEMA, MANIFEST_TEMPLATE, Manifest, Package, ResolvedManifest,
    RuntimeSource, init_manifest,
};
#[doc(hidden)]
pub use path_redaction::redact_absolute_paths;
#[doc(hidden)]
pub use process::{ProcessFailure, ProcessLimits, ProcessOutput};

/// Scans the current manifest's closed grammar directly for fuzz targets.
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub fn fuzz_parse_manifest(source: &str) {
    let _ = Manifest::parse("<fuzz-manifest>", source);
}
