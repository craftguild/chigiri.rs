//! Closed explicit consumer registrations resolved against adopted field identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chigiri_host::PortableRelativePath;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::{EventSettingsError, SourceLimits};
use crate::ir::{FieldId, MockIr};

/// One explicit named consumer registration, including disabled modules.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerRegistration {
    /// Adopted Subscription root field name, independent of the root type name.
    pub field: String,
    /// Whether startup establishes this consumer's source and worker.
    pub enabled: bool,
}

/// Owned event configuration; absent subfields select defaults and no consumers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EventSettings {
    limits: SourceLimits,
    #[serde(deserialize_with = "deserialize_consumers")]
    consumers: BTreeMap<String, ConsumerRegistration>,
}

/// A registered consumer with a verified portable module path and field ID.
///
/// This contains no script, VM, timer or activated registration. Validation
/// can inspect disabled definitions without executing either entrypoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedConsumer {
    name: String,
    field: FieldId,
    enabled: bool,
    path: PortableRelativePath,
}

impl EventSettings {
    /// Inclusive registry capacity, counting disabled registrations as well.
    pub const MAX_CONSUMERS: usize = 1_024;
    /// Inclusive maximum ASCII byte length of a portable consumer identifier.
    pub const MAX_NAME_BYTES: usize = 64;

    /// Validate a host-owned registry before accepting it as configuration.
    pub fn new(
        limits: SourceLimits,
        consumers: BTreeMap<String, ConsumerRegistration>,
    ) -> Result<Self, EventSettingsError> {
        if consumers.len() > Self::MAX_CONSUMERS {
            return Err(EventSettingsError::ConsumerCapacity);
        }
        let mut names = BTreeSet::new();
        for name in consumers.keys() {
            validate_name(name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(EventSettingsError::ConsumerCollision);
            }
        }
        Ok(Self { limits, consumers })
    }

    /// Validated source-stage limits, independent of encoded GraphQL queues.
    pub const fn limits(&self) -> SourceLimits {
        self.limits
    }

    /// Exact registrations in deterministic name order, including disabled ones.
    pub fn consumers(&self) -> &BTreeMap<String, ConsumerRegistration> {
        &self.consumers
    }

    /// Validate every field before any source/module activation or generation.
    pub fn resolve(&self, ir: &MockIr) -> Result<Vec<ResolvedConsumer>, EventSettingsError> {
        let graph = ir.public_graph();
        self.consumers
            .iter()
            .map(|(name, registration)| {
                let root = graph
                    .subscription()
                    .ok_or(EventSettingsError::ConsumerField)?;
                let public = graph.types()[root.0]
                    .fields
                    .get(registration.field.as_str())
                    .ok_or(EventSettingsError::ConsumerField)?;
                // Public and combined graphs can have different numeric IDs.
                // Resolve through the actual public coordinate, never reuse an
                // unchecked public ID as a storage/execution field ID.
                let root_name = &graph.types()[root.0].name;
                let combined_root = ir
                    .graph()
                    .type_id(root_name)
                    .ok_or(EventSettingsError::ConsumerField)?;
                let field_name = &graph.fields()[public.0].name;
                let field = *ir.graph().types()[combined_root.0]
                    .fields
                    .get(field_name)
                    .ok_or(EventSettingsError::ConsumerField)?;
                let path = PortableRelativePath::new(&format!("consumers/{name}.luau"))
                    .map_err(|_| EventSettingsError::ConsumerName)?;
                Ok(ResolvedConsumer {
                    name: name.clone(),
                    field,
                    enabled: registration.enabled,
                    path,
                })
            })
            .collect()
    }
}

impl ResolvedConsumer {
    /// Exact configured name, validated as an ASCII identifier.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Field ID resolved in the combined execution graph from a public coordinate.
    pub const fn field(&self) -> FieldId {
        self.field
    }

    /// Whether the host should establish this consumer at startup.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// The sole allowed logical path; filesystem access still requires no-follow confinement.
    pub fn path(&self) -> &PortableRelativePath {
        &self.path
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), EventSettingsError> {
    let valid_first = |byte: u8| byte.is_ascii_alphabetic() || byte == b'_';
    if name.is_empty()
        || name.len() > EventSettings::MAX_NAME_BYTES
        || !valid_first(name.as_bytes()[0])
        || !name
            .bytes()
            .all(|byte| valid_first(byte) || byte.is_ascii_digit())
    {
        return Err(EventSettingsError::ConsumerName);
    }
    Ok(())
}

fn deserialize_consumers<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, ConsumerRegistration>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Registry;
    impl<'de> Visitor<'de> for Registry {
        type Value = BTreeMap<String, ConsumerRegistration>;

        fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
            out.write_str("a bounded map of uniquely named consumer registrations")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut consumers = BTreeMap::new();
            let mut names = BTreeSet::new();
            while let Some(name) = map.next_key::<String>()? {
                if consumers.len() == EventSettings::MAX_CONSUMERS {
                    return Err(de::Error::custom(EventSettingsError::ConsumerCapacity));
                }
                validate_name(&name).map_err(de::Error::custom)?;
                if !names.insert(name.to_ascii_lowercase()) {
                    return Err(de::Error::custom(EventSettingsError::ConsumerCollision));
                }
                consumers.insert(name, map.next_value()?);
            }
            Ok(consumers)
        }
    }
    deserializer.deserialize_map(Registry)
}
