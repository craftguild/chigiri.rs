//! Validate resolver coverage, aggregate capacity, and selected modules against one production contract.

use super::*;

impl ScriptSet {
    /// All candidates are checked and aligned in ID order.
    pub fn new(contract: Contract, candidates: Vec<ScriptCandidate>) -> Result<Self, ScriptError> {
        Self::with_hooks(contract, candidates, vec![], None).map(|(scripts, _)| scripts)
    }

    pub(crate) fn with_hooks(
        contract: Contract,
        candidates: Vec<ScriptCandidate>,
        hooks: Vec<super::super::HookCandidate>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(Self, Vec<super::super::HookScript>), ScriptError> {
        Self::with_modules(contract, candidates, hooks, vec![], control)
            .map(|(scripts, hooks, _)| (scripts, hooks))
    }

    pub(crate) fn with_modules(
        contract: Contract,
        candidates: Vec<ScriptCandidate>,
        hooks: Vec<super::super::HookCandidate>,
        consumers: Vec<super::ConsumerCandidate>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<
        (
            Self,
            Vec<super::super::HookScript>,
            Vec<super::ConsumerScript>,
        ),
        ScriptError,
    > {
        if candidates.len() != contract.ir.scripts().len() {
            return Err(ScriptError::new("contract", "script coverage mismatch"));
        }
        let mut scripts = contract.prepare(candidates)?;
        let used: usize = scripts.iter().map(Script::validation_bytes).sum();
        let hook_bytes = contract.hook_capacity(
            hooks
                .iter()
                .map(|candidate| (candidate.slot, candidate.source.as_str())),
            MAX_GENERATED - used,
        )?;
        let mut hooks = contract.prepare_hooks(hooks)?;
        let mut consumers =
            contract.prepare_consumers(consumers, MAX_GENERATED - used - hook_bytes)?;
        super::super::validation_batch::validate_with_consumers(
            contract.declarations.clone(),
            &mut scripts,
            &mut hooks,
            &mut consumers,
            control,
        )?;
        Ok((Self { contract, scripts }, hooks, consumers))
    }
    /// Return all validated resolver scripts in coordinate order.
    pub fn scripts(&self) -> &[Script] {
        &self.scripts
    }
    /// Return the IR-derived contract for this set.
    pub fn contract(&self) -> &Contract {
        &self.contract
    }
}

impl Contract {
    /// Validate selected modules with the production syntax, type, initialization, and wrapper budgets.
    /// This path validates selected modules without constructing an executable `ScriptSet`.
    pub fn check_selected(&self, candidates: Vec<ScriptCandidate>) -> Result<(), ScriptError> {
        self.check_selected_with(candidates, None)
    }

    /// Apply the supplied cancellation and absolute deadline to VM admission and initialization.
    pub fn check_selected_with(
        &self,
        candidates: Vec<ScriptCandidate>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(), ScriptError> {
        self.check_selected_and_hooks(candidates, vec![], control)
    }

    pub(crate) fn check_selected_and_hooks(
        &self,
        candidates: Vec<ScriptCandidate>,
        hooks: Vec<super::super::HookCandidate>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(), ScriptError> {
        self.check_selected_modules(candidates, hooks, vec![], control)
    }

    pub(crate) fn check_selected_modules(
        &self,
        candidates: Vec<ScriptCandidate>,
        hooks: Vec<super::super::HookCandidate>,
        consumers: Vec<super::ConsumerCandidate>,
        control: Option<&crate::validation_control::ValidationControl>,
    ) -> Result<(), ScriptError> {
        let mut scripts = self.prepare(candidates)?;
        let used: usize = scripts.iter().map(Script::validation_bytes).sum();
        let hook_bytes = self.hook_capacity(
            hooks.iter().map(|hook| (hook.slot, hook.source.as_str())),
            MAX_GENERATED - used,
        )?;
        let mut hooks = self.prepare_hooks(hooks)?;
        let mut consumers = self.prepare_consumers(consumers, MAX_GENERATED - used - hook_bytes)?;
        super::super::validation_batch::validate_with_consumers(
            self.declarations.clone(),
            &mut scripts,
            &mut hooks,
            &mut consumers,
            control,
        )?;
        Ok(())
    }

    /// Preflight active candidates independently for syntax, type, and aggregate capacity.
    pub(crate) fn check_capacity(
        &self,
        candidates: &[ScriptCandidate],
    ) -> Result<usize, ScriptError> {
        let mut bytes = 0;
        let mut ids = std::collections::BTreeSet::new();
        for candidate in candidates {
            if !ids.insert(candidate.script.0) {
                return Err(ScriptError::new("contract", "duplicate script coordinate"));
            }
            let script = self.build(candidate, MAX_GENERATED - bytes)?;
            bytes += script.source.len()
                + script.declarations.len() * 2
                + script.source().len()
                + "--!strict\n".len();
        }
        Ok(bytes)
    }

    pub(crate) fn prepare(
        &self,
        mut candidates: Vec<ScriptCandidate>,
    ) -> Result<Vec<Script>, ScriptError> {
        if candidates.len() > self.ir.scripts().len() {
            return Err(ScriptError::new("contract", "script coverage mismatch"));
        }
        candidates.sort_by_key(|candidate| candidate.script.0);
        let mut scripts = Vec::new();
        let mut bytes = 0usize;
        let mut previous = None;
        for candidate in candidates {
            if previous == Some(candidate.script) {
                return Err(ScriptError::new(
                    "contract",
                    "duplicate or unknown script coordinate",
                ));
            }
            previous = Some(candidate.script);
            // Pass the remaining aggregate budget so building stops before allocating excess wrapper bytes.
            let id = candidate.script;
            let script = self
                .build(&candidate, MAX_GENERATED - bytes)
                .map_err(|mut error| {
                    error.script = Some(id);
                    error
                })?;
            bytes += script.source.len()
                + script.declarations.len() * 2
                + script.source().len()
                + "--!strict\n".len();
            scripts.push(script);
        }
        Ok(scripts)
    }
}
