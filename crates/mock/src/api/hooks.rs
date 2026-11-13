//! Share each hook phase's API surface across type generation, execution, and reference output.

use super::hook_values::HookValue as V;
use crate::hooks::{HookPhase, HookSlot, HookTiming};

pub(crate) const RUN: &str = "run";
pub(crate) const STATE: &str = "state";
pub(crate) const PATCH: &str = "statePatch";
pub(crate) const REJECT: &str = "reject";
pub(crate) const LOG: &str = "log";
pub(crate) const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

/// Fixed snapshot field name and structure.
pub(crate) struct SnapshotType {
    pub alias: &'static str,
    pub fields: &'static [(&'static str, V)],
}

pub(crate) const SNAPSHOTS: &[SnapshotType] = &[
    SnapshotType {
        alias: "ChigiriHookHeader",
        fields: &[("name", V::Text), ("value", V::Text)],
    },
    SnapshotType {
        alias: "ChigiriHookRequest",
        fields: &[
            ("method", V::Text),
            ("host", V::Text),
            ("port", V::Number),
            ("url", V::Text),
            ("body", V::Text),
            ("headers", V::Array(&V::Named("ChigiriHookHeader"))),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookOperation",
        fields: &[
            (
                "kind",
                V::Enumeration(&["query", "mutation", "subscription"]),
            ),
            ("name", V::Union(&[V::Text, V::Null])),
            ("variables", V::JsonObject),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookField",
        fields: &[
            ("coordinate", V::Text),
            ("path", V::Array(&V::Union(&[V::Text, V::Number]))),
            ("arguments", V::JsonObject),
            ("parent", V::Union(&[V::JsonObject, V::Null])),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookError",
        fields: &[
            ("message", V::Text),
            ("code", V::Text),
            ("path", V::Array(&V::Union(&[V::Text, V::Number]))),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookResponse",
        fields: &[
            ("data", V::Json),
            ("errors", V::Array(&V::Named("ChigiriHookError"))),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookFieldOutcome",
        fields: &[
            ("value", V::Json),
            ("errors", V::Array(&V::Named("ChigiriHookError"))),
        ],
    },
    SnapshotType {
        alias: "ChigiriHookRejectReason",
        fields: &[("code", V::Number), ("message", V::Optional(&V::Text))],
    },
];

pub(crate) fn alias(slot: HookSlot, suffix: &str) -> String {
    let phase = match slot.phase() {
        HookPhase::Request => "Request",
        HookPhase::Operation => "Operation",
        HookPhase::Field => "Field",
    };
    let timing = match slot.timing() {
        HookTiming::Before => "Before",
        HookTiming::After => "After",
    };
    format!("Chigiri{phase}{timing}{suffix}")
}

/// Return phase-specific fields without exposing a raw request after execution.
pub(crate) fn phase_fields(slot: HookSlot) -> Vec<(&'static str, &'static str)> {
    let mut fields = Vec::with_capacity(3);
    if slot.timing() == HookTiming::Before {
        fields.push(("request", "ChigiriHookRequest"));
    } else {
        fields.push((
            "result",
            if slot.phase() == HookPhase::Field {
                "ChigiriHookFieldOutcome"
            } else {
                "ChigiriHookResponse"
            },
        ));
    }
    match slot.phase() {
        HookPhase::Request => {}
        HookPhase::Operation => fields.push(("operation", "ChigiriHookOperation")),
        HookPhase::Field => {
            fields.push(("operation", "ChigiriHookOperation"));
            fields.push(("field", "ChigiriHookField"));
        }
    }
    fields
}
