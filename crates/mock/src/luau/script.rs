//! Validate complete modules independently and map failures back to exact resolver coordinates.

mod consumer;
mod selection;
pub use consumer::{ConsumerCandidate, ConsumerScript, render_consumer_module, static_consumers};

use std::sync::Arc;

use super::text::Text;
use super::{Contract, MAX_BODY, MAX_GENERATED, MAX_WRAPPER, ScriptCandidate, ScriptError};
use crate::ir::{MockIr, ScriptId};

/// One syntax-checked and type-checked resolver with no retained VM handles.
pub struct Script {
    ir: Arc<MockIr>,
    initial_state: Arc<chigiri::core::Object>,
    id: ScriptId,
    source: String,
    coordinate: Arc<str>,
    body_start: usize,
    body_range: std::ops::Range<usize>,
    body_lines: usize,
    declarations: Arc<str>,
    consumer: Option<crate::events::ResolvedConsumer>,
}

/// Script set validated for coordinate coverage and aggregate generated-byte limits.
pub struct ScriptSet {
    contract: Contract,
    scripts: Vec<Script>,
}

impl Contract {
    /// Validate one edited module through standalone syntax/type checks and initialization.
    pub fn script(&self, candidate: ScriptCandidate) -> Result<Script, ScriptError> {
        self.script_with_control(candidate, None)
    }

    pub(crate) fn script_with_control(
        &self,
        candidate: ScriptCandidate,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<Script, ScriptError> {
        let id = candidate.script;
        let result = self
            .build(&candidate, MAX_GENERATED)
            .and_then(|mut script| {
                super::validation_batch::validate(
                    self.declarations.clone(),
                    std::slice::from_mut(&mut script),
                    &mut [],
                    control,
                )?;
                Ok(script)
            });
        result.map_err(|mut error| {
            error.script = Some(id);
            error
        })
    }

    fn build(&self, candidate: &ScriptCandidate, limit: usize) -> Result<Script, ScriptError> {
        self.build_for(candidate, limit, None)
    }

    fn build_for(
        &self,
        candidate: &ScriptCandidate,
        limit: usize,
        consumer: Option<&crate::events::ResolvedConsumer>,
    ) -> Result<Script, ScriptError> {
        let plan = self
            .ir
            .scripts()
            .get(candidate.script.0)
            .ok_or_else(|| ScriptError::new("contract", "unknown script coordinate"))?;
        if candidate.source.len() > MAX_BODY {
            return Err(ScriptError::limit());
        }
        let field = &self.ir.graph().fields()[plan.field.0];
        let parent = &self.ir.graph().types()[field.parent.0];
        let mut out = Text::new(
            MAX_WRAPPER
                .min(limit)
                .saturating_sub(self.declarations.len()),
        );
        out.add("--!strict\n")?;
        out.format(format_args!(
            "local function module(): {}\n",
            super::names::resolver(
                &self.ir,
                candidate.script,
                if consumer.is_some() {
                    "Consumer"
                } else {
                    "Resolver"
                }
            )
        ))?;
        let prefix = out.finish();
        let body_start = prefix.lines().count() + 1;
        let body_range = prefix.len()..prefix.len() + candidate.source.len();
        let mut out = Text::new(
            MAX_WRAPPER
                .min(limit)
                .saturating_sub(self.declarations.len()),
        );
        out.add(&prefix)?;
        out.add(&candidate.source)?;
        out.add("\nend\nreturn module()\n")?;
        let source = out.finish();
        // Charge both the standalone module and contract wrapper before invoking the compiler.
        if source.len() + self.declarations.len() * 2 + candidate.source.len() + "--!strict\n".len()
            > limit
        {
            return Err(ScriptError::limit());
        }
        Ok(Script {
            ir: self.ir.clone(),
            initial_state: self.state.instantiate().snapshot(),
            id: candidate.script,
            coordinate: Arc::from(format!("{}.{}", parent.name, field.name)),
            source,
            body_start,
            body_range,
            body_lines: candidate.source.lines().count().max(1),
            declarations: self.declarations.clone(),
            consumer: consumer.cloned(),
        })
    }
}

impl Script {
    pub(super) fn module_contract(&self) -> super::module_shape::ModuleContract {
        use super::module_shape::ModuleContract;
        if self.consumer.is_some() {
            ModuleContract::Consumer
        } else {
            match self.ir.scripts()[self.id.0].role {
                crate::ir::ScriptRole::SubscriptionRoot { root_as_nested } => {
                    ModuleContract::Subscription { root_as_nested }
                }
                _ => ModuleContract::Ordinary,
            }
        }
    }

    pub(crate) fn validation_bytes(&self) -> usize {
        self.source.len() + self.declarations.len() * 2 + self.source().len() + "--!strict\n".len()
    }
    pub(crate) fn initial_state(&self) -> Arc<chigiri::core::Object> {
        self.initial_state.clone()
    }

    /// Return the schema instance owned by this script; it must not cross into another worker.
    pub fn ir(&self) -> &Arc<MockIr> {
        &self.ir
    }
    /// Return the script's IR location.
    pub fn id(&self) -> ScriptId {
        self.id
    }
    /// Return the complete validated editable module.
    pub fn source(&self) -> &str {
        &self.source[self.body_range.clone()]
    }
    /// Return the canonical GraphQL coordinate.
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub(crate) fn consumer_registration(&self) -> Option<&crate::events::ResolvedConsumer> {
        self.consumer.as_ref()
    }

    pub(crate) fn queue_validation(
        &self,
        control: Option<&crate::validation_control::ValidationControl>,
        batch: &mut super::validation_batch::ValidationBatch,
    ) -> Result<(), ScriptError> {
        if let Some(control) = control {
            control.script()?;
        }
        // Check the standalone module first, then its generated expected-return wrapper.
        luaur::compile(self.source())
            .map_err(|_| self.error("compile", "script syntax or compilation failed", 1, 1))?;
        let standalone = format!("--!strict\n{}", self.source());
        if let Some(control) = control {
            control.script()?;
        }
        self.queue_types(&standalone, 1, batch)?;
        if let Some(control) = control {
            control.script()?;
        }
        self.queue_types(&self.source, self.body_start - 1, batch)?;
        if let Some(control) = control {
            control.script()?;
        }
        Ok(())
    }

    fn queue_types(
        &self,
        source: &str,
        prefix_lines: usize,
        batch: &mut super::validation_batch::ValidationBatch,
    ) -> Result<(), ScriptError> {
        luaur::compile(source)
            .map_err(|_| self.error("compile", "script syntax or compilation failed", 1, 1))?;
        let mut error = self.error("type", "script does not match the resolver contract", 1, 1);
        if self.consumer.is_some() {
            error.expected = "closed consumer module { subscribe: generated function, consume: generated function }".into();
        } else if let crate::ir::ScriptRole::SubscriptionRoot { root_as_nested } =
            self.ir.scripts()[self.id.0].role
        {
            error.expected = if root_as_nested {
                "closed subscription module { subscribe: generated function, resolve: generated function }"
            } else {
                "closed subscription module { subscribe: generated function }"
            }
            .into();
        }
        batch.add(source, prefix_lines, self.body_lines, error, &self.ir)
    }

    pub(crate) fn type_admission_error(&self) -> ScriptError {
        self.error("environment", "type checker admission expired", 1, 1)
    }

    pub(crate) fn initialize(
        &mut self,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(), ScriptError> {
        super::module_value::initialize(self, control)
    }

    /// Return a safe runtime failure without interpreting VM message text as provenance.
    pub fn runtime_error(&self, phase: &'static str) -> ScriptError {
        self.error(phase, "resolver failed", 1, 1)
    }

    /// Map a failure to this module without copying user values or raw error text into diagnostics.
    pub fn error(
        &self,
        phase: &'static str,
        message: &'static str,
        line: usize,
        column: usize,
    ) -> ScriptError {
        let mut error = ScriptError::new(phase, message);
        error.script = Some(self.id);
        error.consumer = self
            .consumer
            .as_ref()
            .map(|consumer| consumer.name().to_owned());
        error.body = (line.max(1).min(self.body_lines), column.max(1));
        error
    }

    pub(crate) fn positioned_error(
        &self,
        phase: &'static str,
        message: &'static str,
        line: usize,
        column: usize,
    ) -> ScriptError {
        let mut error = self.error(phase, message, line, column);
        error.mark_source_position();
        error
    }

    pub(crate) fn output_error(
        &self,
        field: crate::ir::FieldId,
        failure: crate::output::OutputError,
    ) -> ScriptError {
        let mut error = ScriptError::output(&self.ir, field, failure);
        error.script = Some(self.id);
        error
    }
}
