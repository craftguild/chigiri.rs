//! Validate enabled hooks under the same closed module contract and VM budget, retaining owned source.

use std::sync::Arc;

use crate::hooks::HookSlot;
use crate::validation_control::ValidationControl;

use super::text::Text;
use super::{Contract, MAX_BODY, MAX_GENERATED, MAX_WRAPPER, ScriptError};

/// User or bundled hook candidate validated during module initialization.
#[derive(Clone, Debug)]
pub struct HookCandidate {
    /// Fixed hook slot.
    pub slot: HookSlot,
    /// Complete editable module source without generated wrappers.
    pub source: String,
}

/// Type-checked and initialized hook that retains no live VM.
pub struct HookScript {
    ir: Arc<crate::ir::MockIr>,
    contract: crate::hooks::StateContract,
    slot: HookSlot,
    source: String,
}

impl Contract {
    /// Validate unique hook candidates; the project layer enforces enabled-slot coverage.
    pub fn hooks(
        &self,
        candidates: Vec<HookCandidate>,
        control: Option<&ValidationControl>,
    ) -> Result<Vec<HookScript>, ScriptError> {
        let mut scripts = self.prepare_hooks(candidates)?;
        super::validation_batch::validate(
            self.declarations.clone(),
            &mut [],
            &mut scripts,
            control,
        )?;
        Ok(scripts)
    }

    pub(crate) fn prepare_hooks(
        &self,
        mut candidates: Vec<HookCandidate>,
    ) -> Result<Vec<HookScript>, ScriptError> {
        candidates.sort_by_key(|candidate| candidate.slot);
        self.hook_capacity(
            candidates
                .iter()
                .map(|candidate| (candidate.slot, candidate.source.as_str())),
            MAX_GENERATED,
        )?;
        let mut scripts = Vec::new();
        for candidate in candidates {
            scripts.push(HookScript {
                ir: self.ir.clone(),
                contract: self.state.clone(),
                slot: candidate.slot,
                source: candidate.source,
            });
        }
        Ok(scripts)
    }
    /// Charge hook sources, including supplied disabled modules, against the remaining aggregate budget.
    pub(crate) fn hook_capacity<'a>(
        &self,
        sources: impl IntoIterator<Item = (HookSlot, &'a str)>,
        remaining: usize,
    ) -> Result<usize, ScriptError> {
        let mut slots = std::collections::BTreeSet::new();
        let mut bytes = 0;
        // Check aggregate candidate capacity before any candidate's syntax can mask overflow.
        for (slot, source) in sources {
            if !slots.insert(slot) || slots.len() > HookSlot::ALL.len() {
                return Err(ScriptError::new("contract", "duplicate hook slot"));
            }
            if source.len() > MAX_BODY {
                return Err(ScriptError::limit());
            }
            let wrapper = wrapped(slot, source)?;
            let size = self.declarations.len() * 2 + wrapper.len() + source.len() + 10;
            if self.declarations.len() + wrapper.len() > MAX_WRAPPER
                || size > remaining.saturating_sub(bytes)
            {
                return Err(ScriptError::limit());
            }
            bytes += size;
        }
        Ok(bytes)
    }
}

impl HookScript {
    pub(crate) fn accepts(
        &self,
        snapshot: &crate::store::Snapshot,
        state: &crate::hooks::RequestState,
    ) -> bool {
        Arc::ptr_eq(&self.ir, snapshot.ir()) && self.contract.owns(state)
    }
    /// Return the slot recorded by the validated candidate, without inferring it from source text.
    pub fn slot(&self) -> HookSlot {
        self.slot
    }
    /// Return source bytes unchanged for export.
    pub fn source(&self) -> &str {
        &self.source
    }
    pub(crate) fn error(&self, phase: &'static str, message: &'static str) -> ScriptError {
        let mut error = ScriptError::new(phase, message);
        error.hook = Some(self.slot);
        error
    }

    pub(crate) fn queue_validation(
        &self,
        control: Option<&ValidationControl>,
        batch: &mut super::validation_batch::ValidationBatch,
    ) -> Result<(), ScriptError> {
        if let Some(control) = control {
            control.script()?;
        }
        // Validate standalone syntax before checking the complete contract wrapper.
        luaur::compile(self.source())
            .map_err(|_| self.error("compile", "hook syntax or compilation failed"))?;
        for (source, prefix) in [
            (format!("--!strict\n{}", self.source()), 1),
            (wrapped(self.slot, self.source())?, 2),
        ] {
            if let Some(control) = control {
                control.script()?;
            }
            batch.add(
                &source,
                prefix,
                self.source.lines().count().max(1),
                self.error("type", "hook does not match the slot contract"),
                &self.ir,
            )?;
        }
        Ok(())
    }

    pub(crate) fn type_admission_error(&self) -> ScriptError {
        self.error("environment", "hook type checker admission expired")
    }

    pub(crate) fn initialize(
        &mut self,
        control: Option<&ValidationControl>,
    ) -> Result<(), ScriptError> {
        super::hook_value::initialize(self, control)
    }
}

fn wrapped(slot: HookSlot, source: &str) -> Result<String, ScriptError> {
    let mut out = Text::new(MAX_WRAPPER);
    out.format(format_args!(
        "--!strict\nlocal function module(): {}\n",
        crate::api::hooks::alias(slot, "Hook")
    ))?;
    out.add(source)?;
    out.add("\nend\nreturn module()\n")?;
    Ok(out.finish())
}
