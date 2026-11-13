//! Redacted configuration identity plus explicitly allowlisted numeric evidence.

use std::fmt;

use super::{ConfigurationConstraint, ConfigurationSlot, ConfigurationViolation};
use crate::configuration::ConfigurationOrigin;

/// Final validation diagnostic with no secrets or arbitrary application text.
///
/// Numeric subscription delivery/protocol settings may include bounded
/// [`ConfigurationConstraint`] evidence. Other slots expose only identities.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConfigurationDiagnostic {
    slot: ConfigurationSlot,
    origin: ConfigurationOrigin,
    violation: ConfigurationViolation,
    constraint: Option<ConfigurationConstraint>,
}

impl ConfigurationDiagnostic {
    pub(crate) const fn new(
        slot: ConfigurationSlot,
        origin: ConfigurationOrigin,
        violation: ConfigurationViolation,
    ) -> Self {
        Self {
            slot,
            origin,
            violation,
            constraint: None,
        }
    }

    pub(crate) const fn with_constraint(mut self, constraint: ConfigurationConstraint) -> Self {
        // This closed allowlist prevents a future general-purpose build-error
        // caller from accidentally disclosing another slot's numeric values.
        use ConfigurationSlot::*;
        if matches!(
            self.slot,
            ActiveSubscriptions
                | QueuedSubscriptionEvents
                | QueuedSubscriptionBytes
                | EncodedSubscriptionEventBytes
                | SubscriptionQueueAge
                | SubscriptionSendNoProgressTimeout
                | SubscriptionSetupTimeout
                | SubscriptionEventTimeout
                | EndpointSubscriptionEncodedBytes
                | PendingOperationSetups
                | RetainedSubscriberMetadata
                | SubscriptionHeartbeatInterval
                | ActiveOperationsPerConnection
                | WebSocketConnections
                | PendingSetupsPerConnection
        ) {
            self.constraint = Some(constraint);
        }
        self
    }

    /// Returns the setting whose final validation failed.
    pub const fn slot(self) -> ConfigurationSlot {
        self.slot
    }
    /// Returns the source of that setting's final value.
    pub const fn origin(self) -> ConfigurationOrigin {
        self.origin
    }
    /// Returns the violated rule's classification.
    pub const fn violation(self) -> ConfigurationViolation {
        self.violation
    }
    /// Returns numeric evidence only for subscription delivery/protocol settings.
    pub const fn constraint(self) -> Option<ConfigurationConstraint> {
        self.constraint
    }
}

impl fmt::Debug for ConfigurationDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut diagnostic = formatter.debug_struct("ConfigurationDiagnostic");
        diagnostic
            .field("slot", &self.slot)
            .field("origin", &self.origin)
            .field("violation", &self.violation);
        if let Some(constraint) = self.constraint {
            diagnostic.field("constraint", &constraint);
        }
        diagnostic.finish()
    }
}

impl fmt::Display for ConfigurationDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}
