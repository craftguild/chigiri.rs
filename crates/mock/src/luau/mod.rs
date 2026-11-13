//! Generate Luau contracts from shared IR and validate static candidates like edited modules.

mod candidate;
mod consumer_types;
mod declarations;
mod event_contract;
pub(crate) use event_contract::{publication_contract, source_contract};
pub mod format;
mod hook_script;
mod hook_types;
mod hook_value;
mod literal_precision;
mod literal_precision_sites;
mod literal_precision_sources;
mod literal_precision_syntax;
#[cfg(test)]
mod literal_precision_tests;
mod literal_precision_types;
mod literal_precision_visit;
mod literal_string;
mod method_signatures;
mod methods;
mod models;
mod module_contract;
mod module_shape;
mod module_value;
mod names;
mod query;
mod reference;
mod script;
mod subscription;
mod terminal_flow;
mod terminal_flow_visit;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_authoring_boundaries;
#[cfg(test)]
mod tests_authoring_guidance;
#[cfg(test)]
mod tests_consumers;
#[cfg(test)]
mod tests_domains;
#[cfg(test)]
mod tests_event_sequences;
#[cfg(test)]
mod tests_event_sources;
#[cfg(test)]
mod tests_format;
#[cfg(test)]
mod tests_hook_capacity;
#[cfg(test)]
mod tests_hooks;
#[cfg(test)]
mod tests_inputs;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_literal_strings;
#[cfg(test)]
mod tests_module_values;
#[cfg(test)]
mod tests_modules;
#[cfg(test)]
mod tests_names;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod tests_relations;
#[cfg(test)]
mod tests_root_scope;
#[cfg(test)]
mod tests_subscription_types;
#[cfg(test)]
mod tests_subscriptions;
#[cfg(test)]
mod tests_terminal_flow;
#[cfg(test)]
mod tests_terminal_flow_hygiene;
#[cfg(test)]
mod tests_validation_batch;
pub(crate) mod text;
mod validation_batch;

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::ir::{MockIr, ScriptId};

pub use candidate::{ScriptCandidate, static_candidates};
pub use hook_script::{HookCandidate, HookScript};
pub use hook_types::{render_hook_module, static_hooks};
pub use module_contract::render_module;
pub(crate) use module_value::load_consumer_observed;
pub(crate) use module_value::load_observed as load_module_observed;
pub(crate) use module_value::load_subscription_observed;
pub use script::{
    ConsumerCandidate, ConsumerScript, Script, ScriptSet, render_consumer_module, static_consumers,
};
pub use subscription::render_subscription_module;
pub(crate) use validation_batch::checker_prelude;

/// Maximum size in bytes of one editable resolver module.
pub const MAX_BODY: usize = 32 * 1024;
/// Maximum size in bytes of one generated wrapper, including shared declarations.
pub const MAX_WRAPPER: usize = 1024 * 1024;
/// Maximum combined size in bytes of generated wrappers and candidates.
pub const MAX_GENERATED: usize = 16 * 1024 * 1024;

/// Fixed, value-free diagnostic details shared by the public error types.
#[derive(Clone, Debug)]
pub struct DiagnosticDetails {
    /// Registered consumer name when this diagnostic belongs to its module.
    pub consumer: Option<String>,
    /// Stable failure phase such as compile, type, or contract.
    pub phase: &'static str,
    /// Stable machine classification independent of compiler or VM wording.
    pub cause: &'static str,
    /// Schema-derived contract member when the checker can identify one safely.
    pub member: Option<String>,
    /// Normalized contract shape expected at the failure site.
    pub expected: String,
    /// Normalized observed shape. It never contains a source literal or value.
    pub actual: String,
    /// Bounded corrective guidance selected from a fixed vocabulary.
    pub hint: String,
    /// Fixed sanitized message.
    pub message: &'static str,
}

/// Script diagnostic that retains neither source text nor runtime values.
#[derive(Clone, Debug)]
pub struct ScriptError {
    /// Sanitized logical stack, absent when no VM stack was captured.
    pub stack: Option<ScriptStack>,
    /// Whether job cancellation interrupted validation.
    pub(crate) cancelled: bool,
    /// Corresponding resolver script, when identifiable.
    pub script: Option<ScriptId>,
    /// Hook slot when this diagnostic belongs to a hook rather than a resolver.
    pub hook: Option<crate::hooks::HookSlot>,
    /// One-based location in the editable module body.
    pub body: (usize, usize),
    /// Verified source position for runtime logs; absent when no source site is known.
    source_position: Option<(usize, usize)>,
    details: Box<DiagnosticDetails>,
}

impl ScriptError {
    pub(crate) fn invalid_rejection() -> Self {
        Self::new("hook", "invalid hook rejection")
    }

    pub(crate) fn new(phase: &'static str, message: &'static str) -> Self {
        let (cause, expected, actual, hint) = diagnostic_shape(phase);
        Self {
            stack: None,
            cancelled: false,
            script: None,
            hook: None,
            body: (1, 1),
            source_position: None,
            details: Box::new(DiagnosticDetails {
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

    pub(crate) fn output(
        ir: &MockIr,
        field: crate::ir::FieldId,
        failure: crate::output::OutputError,
    ) -> Self {
        if failure.is_resource_limit() {
            return Self::new("budget", "output resource limit");
        }
        let diagnostic = failure.diagnostic(ir, field);
        let mut error = Self::new("output", "resolver output validation failed");
        error.cause = diagnostic.cause;
        error.member = Some(diagnostic.member);
        error.expected = diagnostic.expected;
        error.actual = diagnostic.actual.into();
        error.hint = diagnostic.hint.into();
        error
    }
    fn limit() -> Self {
        Self::new("limit", "Luau generation byte limit")
    }

    pub(crate) fn mark_source_position(&mut self) {
        self.source_position = Some(self.body);
    }

    pub(crate) fn source_position(&self) -> Option<(usize, usize)> {
        self.source_position
    }
}

impl Deref for ScriptError {
    type Target = DiagnosticDetails;

    fn deref(&self) -> &Self::Target {
        &self.details
    }
}

impl DerefMut for ScriptError {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.details
    }
}

/// Return the fixed safe diagnostic vocabulary for a failure phase.
pub fn diagnostic_shape(phase: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match phase {
        "compile" => (
            "MOCK_LUAU_SYNTAX",
            "valid Luau module",
            "syntax error",
            "Fix the syntax at the reported logical position.",
        ),
        "type" => (
            "MOCK_CONTRACT",
            "valid typed module",
            "type detail unavailable",
            "Review the generated declarations and reported logical position.",
        ),
        "module" => (
            "MOCK_MODULE_CONTRACT",
            "closed module descriptor",
            "invalid module descriptor",
            "Return the generated closed module shape.",
        ),
        "limit" | "budget" | "timeout" | "cancelled" => (
            "MOCK_RESOURCE_LIMIT",
            "bounded execution",
            "resource limit reached",
            "Reduce the operation or configured input size.",
        ),
        "business" => (
            "MOCK_BUSINESS_REJECTION",
            "successful business operation",
            "explicit rejection",
            "Handle the published business error code.",
        ),
        "environment" => (
            "MOCK_ENVIRONMENT",
            "available execution environment",
            "environment failure",
            "Retry after correcting the local environment.",
        ),
        _ => (
            "MOCK_CONTRACT",
            "valid contract",
            "contract violation",
            "Check the generated contract.",
        ),
    }
}

/// Bounded stack metadata that omits values, function names, and source text.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScriptStack {
    /// Host / interrupt / reentry receiving boundary.
    pub origin: &'static str,
    /// The row number in the module.
    pub lines: Vec<usize>,
    /// Whether additional frames were removed to enforce the bound.
    pub truncated: bool,
}
impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Luau {}:{} [{}] {}",
            self.body.0, self.body.1, self.phase, self.message
        )
    }
}
impl std::error::Error for ScriptError {}

/// Generate common IR declarations once and share them across resolver coordinates.
pub struct Contract {
    ir: Arc<MockIr>,
    declarations: Arc<str>,
    state: crate::hooks::StateContract,
}

impl Contract {
    /// Generate stable shared declarations from the validated IR.
    pub fn new(ir: Arc<MockIr>) -> Result<Self, ScriptError> {
        Self::with_state(ir, crate::hooks::StateContract::default())
    }

    /// Generate declarations from the explicit state contract, never by inferring types from its initial value.
    pub fn with_state(
        ir: Arc<MockIr>,
        state: crate::hooks::StateContract,
    ) -> Result<Self, ScriptError> {
        crate::api::validate(crate::api::METHODS)
            .map_err(|message| ScriptError::new("contract", message))?;
        names::validate(&ir)?;
        let source = declarations::generate(&ir, &state)?;
        let mut aliases = text::Text::new(MAX_WRAPPER);
        // Definition files and the CLI share this exported alias set, including host-facing named types.
        for line in source.split_inclusive('\n') {
            if line.starts_with("type ") {
                aliases.add("export ")?;
            }
            aliases.add(line)?;
        }
        let aliases = format::source(&aliases.finish(), MAX_WRAPPER)?;
        let mut output = text::Text::new(MAX_WRAPPER);
        output.add(HOST_DEFINITIONS)?;
        subscription::declarations::host(&mut output, &ir)?;
        output.add(&aliases)?;
        let declarations = Arc::from(output.finish());
        Ok(Self {
            ir,
            declarations,
            state,
        })
    }
    /// Return the same IR instance used to derive all expected types.
    pub fn ir(&self) -> &Arc<MockIr> {
        &self.ir
    }
    /// Return the complete non-executable Luau declaration file with host types and public aliases.
    pub fn declarations(&self) -> &str {
        &self.declarations
    }

    /// Return the request-state contract used to generate declarations.
    pub fn state(&self) -> &crate::hooks::StateContract {
        &self.state
    }
}

/// Host null is a nominal type that user table literals cannot forge.
const HOST_DEFINITIONS: &str = include_str!("host.d.luau");

pub(crate) fn capability_declarations() -> Result<String, ScriptError> {
    declarations::capability_declarations()
}
