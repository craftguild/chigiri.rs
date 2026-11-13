//! Define fixed hook slots shared by type generation, execution, and project layout.

use serde::{Deserialize, Serialize};

/// Three lifecycle phases exposed to hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    /// Request parsing and admission phase.
    Request,
    /// Operation phase after selection and variable coercion.
    Operation,
    /// Field invocation after argument coercion.
    Field,
}

/// Hook timing; only `Before` hooks may reject or patch state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTiming {
    /// Before the host advances to the next phase.
    Before,
    /// After the phase has produced its result.
    After,
}

/// Six fixed hook slots, each accepting at most one module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HookSlot {
    /// May reject the request or return a state patch.
    #[serde(rename = "request.before")]
    RequestBefore,
    /// Observes the request-phase result.
    #[serde(rename = "request.after")]
    RequestAfter,
    /// May reject the operation or return a state patch.
    #[serde(rename = "operation.before")]
    OperationBefore,
    /// Observes the operation result.
    #[serde(rename = "operation.after")]
    OperationAfter,
    /// May reject the field; request state is unchanged.
    #[serde(rename = "field.before")]
    FieldBefore,
    /// Observes the resolved field before child selection.
    #[serde(rename = "field.after")]
    FieldAfter,
}

impl HookSlot {
    /// Stable declaration order; parallel field completion does not depend on this order.
    pub const ALL: [Self; 6] = [
        Self::RequestBefore,
        Self::RequestAfter,
        Self::OperationBefore,
        Self::OperationAfter,
        Self::FieldBefore,
        Self::FieldAfter,
    ];

    /// Stable slot name used for registration and diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Self::RequestBefore => "request.before",
            Self::RequestAfter => "request.after",
            Self::OperationBefore => "operation.before",
            Self::OperationAfter => "operation.after",
            Self::FieldBefore => "field.before",
            Self::FieldAfter => "field.after",
        }
    }

    /// Return the single canonical project path for this slot.
    pub const fn path(self) -> &'static str {
        match self {
            Self::RequestBefore => "hooks/request/before.luau",
            Self::RequestAfter => "hooks/request/after.luau",
            Self::OperationBefore => "hooks/operation/before.luau",
            Self::OperationAfter => "hooks/operation/after.luau",
            Self::FieldBefore => "hooks/field/before.luau",
            Self::FieldAfter => "hooks/field/after.luau",
        }
    }

    /// Return the plugin phase for this slot.
    pub const fn phase(self) -> HookPhase {
        match self {
            Self::RequestBefore | Self::RequestAfter => HookPhase::Request,
            Self::OperationBefore | Self::OperationAfter => HookPhase::Operation,
            Self::FieldBefore | Self::FieldAfter => HookPhase::Field,
        }
    }

    /// Return whether this slot runs before or after its phase.
    pub const fn timing(self) -> HookTiming {
        match self {
            Self::RequestBefore | Self::OperationBefore | Self::FieldBefore => HookTiming::Before,
            _ => HookTiming::After,
        }
    }

    /// Return whether this slot may return a state patch.
    pub const fn allows_patch(self) -> bool {
        matches!(self, Self::RequestBefore | Self::OperationBefore)
    }

    /// Every before hook may produce a sticky rejection.
    pub const fn allows_reject(self) -> bool {
        matches!(self.timing(), HookTiming::Before)
    }
}

/// Registration for both timings of one hook phase; serde rejects missing or unknown fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    /// Enable the phase's before hook.
    pub before: bool,
    /// Enable the phase's after hook.
    pub after: bool,
}

/// Closed registration for all hook phases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSettings {
    /// Request-phase registration.
    pub request: Registration,
    /// Operation-phase registration.
    pub operation: Registration,
    /// Field-phase registration.
    pub field: Registration,
}

impl HookSettings {
    /// Return whether the exact slot is enabled for execution, readiness, and file selection.
    pub const fn enabled(&self, slot: HookSlot) -> bool {
        let phase = match slot.phase() {
            HookPhase::Request => self.request,
            HookPhase::Operation => self.operation,
            HookPhase::Field => self.field,
        };
        match slot.timing() {
            HookTiming::Before => phase.before,
            HookTiming::After => phase.after,
        }
    }
}
