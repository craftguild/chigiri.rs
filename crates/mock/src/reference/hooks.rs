//! Add host-surface descriptions plus authorization and registration state for all six hook slots.

use chigiri_host::PortableRelativePath;
use serde::{Deserialize, Serialize};

use super::{ReferenceDocument, ReferenceError, api_types::*, budget::Budget};
use crate::api::{Effect, hook_rules, hooks as api};
use crate::hooks::{HookPhase, HookSettings, HookSlot, HookTiming, StateContract};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HookDoc {
    pub slot: HookSlot,
    pub phase: HookPhase,
    pub timing: HookTiming,
    pub module_path: PortableRelativePath,
    pub enabled: Option<bool>,
    pub context_type_id: String,
    pub result_type_id: String,
    pub module_type_id: String,
    pub state_patch: bool,
    pub reject: bool,
    pub models_read_only: bool,
    pub example_id: String,
}

pub(super) fn append(
    doc: &mut ReferenceDocument,
    budget: &mut Budget,
    configuration: Option<(&StateContract, &HookSettings)>,
) -> Result<(), ReferenceError> {
    super::state_graph::append(doc, budget, configuration.map(|(state, _)| state))?;
    super::hook_graph::append(doc, budget)?;
    for slot in HookSlot::ALL {
        let example = format!("hook-example/{}", slot.name());
        budget.push(
            &mut doc.hooks,
            HookDoc {
                slot,
                phase: slot.phase(),
                timing: slot.timing(),
                module_path: PortableRelativePath::new(slot.path()).expect("fixed hook path"),
                enabled: configuration.map(|(_, settings)| settings.enabled(slot)),
                context_type_id: api::alias(slot, "Context"),
                result_type_id: api::alias(slot, "Result"),
                module_type_id: api::alias(slot, "Hook"),
                state_patch: slot.allows_patch(),
                reject: slot.allows_reject(),
                models_read_only: true,
                example_id: example.clone(),
            },
        )?;
        budget.push(
            &mut doc.examples,
            ExampleDoc {
                id: example,
                title: format!("{} hook (body example)", slot.name()),
                origin: ExampleOrigin::GenericIllustration,
                scope: Effect::Read,
                luau: hook_rules::example(slot).into(),
                rule_ids: hook_rules::RULES
                    .iter()
                    .filter(|(_, _, example, _)| *example == slot)
                    .map(|(id, _, _, _)| (*id).into())
                    .collect(),
            },
        )?;
    }
    for (id, description, example, failure) in hook_rules::RULES {
        budget.push(
            &mut doc.rules,
            RuleDoc {
                id: (*id).into(),
                description: (*description).into(),
                failure_class: *failure,
                example_ids: vec![format!("hook-example/{}", example.name())],
            },
        )?;
    }
    for (id, value, unit, scope, description) in [
        (
            "hook.observation_text_bytes",
            crate::limits::HOOK_OBSERVATION_TEXT_BYTES,
            LimitUnit::Bytes,
            "snapshot_text",
            "Read-only observation text budget shared in call order; log, state, and store views each have separate 4 KiB limits.",
        ),
        (
            "log.record_bytes",
            crate::logging::RECORD_BYTES,
            LimitUnit::Bytes,
            "record",
            "JSONL record including redirects.",
        ),
        (
            "log.hook_queue",
            crate::logging::QUEUE_RECORDS,
            LimitUnit::Items,
            "hook",
            "Maximum number of queued hook log records.",
        ),
        (
            "log.access_queue",
            crate::logging::QUEUE_RECORDS,
            LimitUnit::Items,
            "access",
            "Maximum number of queued access log records.",
        ),
    ] {
        budget.push(
            &mut doc.limits,
            LimitDoc {
                id: id.into(),
                value,
                unit,
                shared_scope: scope.into(),
                description: description.into(),
            },
        )?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RejectionDoc {
    accepted_statuses: Vec<u16>,
    bodyless_statuses: Vec<u16>,
    invalid_status: u16,
}
impl RejectionDoc {
    pub fn build() -> Self {
        Self {
            accepted_statuses: (200..=599)
                .filter(|status| crate::hooks::assigned(*status))
                .collect(),
            bodyless_statuses: Vec::new(),
            invalid_status: crate::hooks::Rejection::invalid().status,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoggingDoc {
    #[serde(deserialize_with = "current_log")]
    pub(super) format: String,
    pub(super) timestamp_unit: String,
    pub(super) duration_clock: String,
    pub(super) identity_scope: String,
    pub(super) diagnostic_fields: Vec<String>,
    pub(super) source_positions_nullable: bool,
}

fn current_log<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<String, D::Error> {
    use serde::Deserialize;
    let value = String::deserialize(reader)?;
    if value != crate::logging::FORMAT {
        return Err(serde::de::Error::custom("unsupported runtime log format"));
    }
    Ok(value)
}
impl LoggingDoc {
    pub fn build() -> Self {
        Self {
            format: crate::logging::FORMAT.into(),
            timestamp_unit: "unix_epoch_milliseconds_utc".into(),
            duration_clock: "monotonic".into(),
            identity_scope: "mock_request".into(),
            diagnostic_fields: [
                "request_id",
                "coordinate",
                "module",
                "phase",
                "cause",
                "member",
                "expected",
                "actual",
                "hint",
                "line",
                "column",
                "stack",
                "cancelled",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            source_positions_nullable: true,
        }
    }
}
