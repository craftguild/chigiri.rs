//! Validate registered two-entrypoint consumers without running either callback.

use super::*;
use crate::events::{EventSettings, ResolvedConsumer};
use crate::ir::FieldPlan;
use crate::validation_control::ValidationControl;

/// One explicitly registered editable consumer module, enabled or disabled.
#[derive(Clone)]
pub struct ConsumerCandidate {
    /// Validated registration and its sole portable module path.
    pub registration: ResolvedConsumer,
    /// Complete authored module, without a generated checking wrapper.
    pub source: String,
}

/// Type-checked consumer source and registration; contains no live VM.
pub struct ConsumerScript {
    script: Script,
}

impl ConsumerScript {
    /// Registration retained exactly, including disabled state.
    pub fn registration(&self) -> &ResolvedConsumer {
        self.script
            .consumer
            .as_ref()
            .expect("consumer script registration")
    }

    /// Original authored bytes for portable export and future short VMs.
    pub fn source(&self) -> &str {
        self.script.source()
    }

    pub(crate) fn script(&self) -> &Script {
        &self.script
    }

    pub(crate) fn initialize(
        &mut self,
        control: Option<&ValidationControl>,
    ) -> Result<(), ScriptError> {
        self.script.initialize(control)
    }
}

impl Contract {
    /// Check all supplied registrations, including disabled callback bodies.
    /// Only module initialization runs; neither callback is invoked.
    pub fn consumers(
        &self,
        candidates: Vec<ConsumerCandidate>,
        control: Option<&ValidationControl>,
    ) -> Result<Vec<ConsumerScript>, ScriptError> {
        let mut consumers = self.prepare_consumers(candidates, MAX_GENERATED)?;
        super::super::validation_batch::validate_with_consumers(
            self.declarations.clone(),
            &mut [],
            &mut [],
            &mut consumers,
            control,
        )?;
        Ok(consumers)
    }

    pub(crate) fn prepare_consumers(
        &self,
        mut candidates: Vec<ConsumerCandidate>,
        remaining: usize,
    ) -> Result<Vec<ConsumerScript>, ScriptError> {
        if candidates.len() > EventSettings::MAX_CONSUMERS {
            return Err(ScriptError::limit());
        }
        candidates.sort_by(|a, b| a.registration.name().cmp(b.registration.name()));
        let mut names = std::collections::BTreeSet::new();
        let mut used = 0;
        let mut consumers = Vec::new();
        for candidate in candidates {
            let registration = candidate.registration;
            let result = (|| {
                if !names.insert(registration.name().to_ascii_lowercase()) {
                    return Err(ScriptError::new(
                        "contract",
                        "duplicate consumer registration",
                    ));
                }
                let Some(FieldPlan::Script(id)) = self.ir.field_plan(registration.field()) else {
                    return Err(ScriptError::new(
                        "contract",
                        "consumer subscription field required",
                    ));
                };
                if !self.ir.scripts()[id.0].role.is_subscription_root() {
                    return Err(ScriptError::new(
                        "contract",
                        "consumer subscription field required",
                    ));
                }
                let script = self.build_for(
                    &ScriptCandidate {
                        script: *id,
                        source: candidate.source,
                    },
                    remaining.saturating_sub(used),
                    Some(&registration),
                )?;
                used += script.validation_bytes();
                Ok(ConsumerScript { script })
            })();
            consumers.push(result.map_err(|mut error| {
                error.consumer = Some(registration.name().to_owned());
                error
            })?);
        }
        Ok(consumers)
    }
}

/// Render the field's exact two-function module without GraphQL setup arguments.
pub fn render_consumer_module(
    ir: &MockIr,
    field: crate::ir::FieldId,
    subscribe_body: &str,
    consume_body: &str,
) -> Result<String, ScriptError> {
    let Some(FieldPlan::Script(id)) = ir.field_plan(field) else {
        return Err(ScriptError::new(
            "contract",
            "consumer subscription field required",
        ));
    };
    if !ir.scripts()[id.0].role.is_subscription_root() {
        return Err(ScriptError::new(
            "contract",
            "consumer subscription field required",
        ));
    }
    let name = |suffix| super::super::names::resolver(ir, *id, suffix);
    let mut out = Text::new(MAX_BODY);
    out.format(format_args!(
        "--!strict\nlocal consumer: {} = {{\n    subscribe = function(ctx: {}): {}\n",
        name("Consumer"),
        name("ConsumerSetupContext"),
        name("Source")
    ))?;
    out.add(subscribe_body)?;
    out.format(format_args!(
        "\n    end,\n    consume = function(ctx: {}, event: {}): ()\n",
        name("ConsumerContext"),
        name("Event")
    ))?;
    out.add(consume_body)?;
    out.add("\n    end,\n}\nreturn consumer\n")?;
    super::super::format::source(&out.finish(), MAX_BODY)
}

/// Generate intentional error scaffolds for every explicit registration.
pub fn static_consumers(
    ir: &MockIr,
    settings: &EventSettings,
) -> Result<Vec<ConsumerCandidate>, ScriptError> {
    settings
        .resolve(ir)
        .map_err(|_| ScriptError::new("contract", "consumer registration field mismatch"))?
        .into_iter()
        .map(|registration| {
            let source = render_consumer_module(
                ir,
                registration.field(),
                "ctx.fail({code = \"NOT_IMPLEMENTED\"})",
                "ctx.fail({code = \"NOT_IMPLEMENTED\"})",
            )?;
            Ok(ConsumerCandidate {
                registration,
                source,
            })
        })
        .collect()
}
