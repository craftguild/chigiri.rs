//! Operation-cost policies use checked arithmetic instead of saturation.

use std::collections::BTreeMap;
use std::fmt;

use chigiri_rate::{OperationCost, OperationCostError};
use chigiri_runtime::{CostArgumentPresence, CostFieldMetadata, OperationIdentity};

/// A validated structural summary of an operation used for cost calculation.
#[derive(Clone, Copy)]
pub struct OperationCostContext<'a> {
    identity: &'a OperationIdentity,
    expanded_fields: u64,
    aliases: u64,
    depth: u64,
    catalog_override: Option<OperationCost>,
    fields: &'a [CostFieldMetadata],
}

impl<'a> OperationCostContext<'a> {
    /// The executor creates context from bounded structural metadata.
    pub const fn new(
        identity: &'a OperationIdentity,
        expanded_fields: u64,
        aliases: u64,
        depth: u64,
        catalog_override: Option<OperationCost>,
        fields: &'a [CostFieldMetadata],
    ) -> Self {
        Self {
            identity,
            expanded_fields,
            aliases,
            depth,
            catalog_override,
            fields,
        }
    }

    pub const fn identity(&self) -> &'a OperationIdentity {
        self.identity
    }
    pub const fn expanded_fields(&self) -> u64 {
        self.expanded_fields
    }
    pub const fn aliases(&self) -> u64 {
        self.aliases
    }
    pub const fn depth(&self) -> u64 {
        self.depth
    }
    pub const fn catalog_override(&self) -> Option<OperationCost> {
        self.catalog_override
    }
    pub const fn fields(&self) -> &'a [CostFieldMetadata] {
        self.fields
    }
}

/// Object-safe policy that computes a deterministic cost for a validated operation.
pub trait OperationCostPolicy: Send + Sync + 'static {
    fn cost(&self, context: OperationCostContext<'_>) -> Result<OperationCost, OperationCostError>;
}

/// Standard policy that charges one unit per expanded field.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnitFieldCost;

impl OperationCostPolicy for UnitFieldCost {
    fn cost(&self, context: OperationCostContext<'_>) -> Result<OperationCost, OperationCostError> {
        if let Some(cost) = context.catalog_override() {
            return Ok(cost);
        }
        OperationCost::new(context.expanded_fields().max(1))
    }
}

/// An invalid or duplicate coordinate/list-rule configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostPolicyConfigurationError;

impl fmt::Display for CostPolicyConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation cost policy configuration is invalid")
    }
}

impl std::error::Error for CostPolicyConfigurationError {}

#[derive(Clone, Debug)]
struct ListMultiplier {
    argument: String,
    maximum: u64,
}

/// Standard policy combining unit costs, coordinate weights, bounded list
/// multipliers, and catalog overrides.
#[derive(Clone, Debug, Default)]
pub struct StandardOperationCost {
    coordinate_weights: BTreeMap<String, OperationCost>,
    list_multipliers: BTreeMap<String, ListMultiplier>,
}

impl StandardOperationCost {
    pub const fn new() -> Self {
        Self {
            coordinate_weights: BTreeMap::new(),
            list_multipliers: BTreeMap::new(),
        }
    }

    /// Sets a fixed weight for a schema coordinate.
    pub fn with_coordinate_weight(
        mut self,
        coordinate: impl Into<String>,
        weight: OperationCost,
    ) -> Result<Self, CostPolicyConfigurationError> {
        let coordinate = coordinate.into();
        validate_coordinate(&coordinate)?;
        if self.coordinate_weights.insert(coordinate, weight).is_some() {
            return Err(CostPolicyConfigurationError);
        }
        Ok(self)
    }

    /// Uses an optional `Int` argument as a list multiplier clamped to `1..=maximum`.
    pub fn with_list_multiplier(
        mut self,
        coordinate: impl Into<String>,
        argument: impl Into<String>,
        maximum: u64,
    ) -> Result<Self, CostPolicyConfigurationError> {
        let coordinate = coordinate.into();
        let argument = argument.into();
        validate_coordinate(&coordinate)?;
        if maximum == 0 || !graphql_name(&argument) {
            return Err(CostPolicyConfigurationError);
        }
        let rule = ListMultiplier { argument, maximum };
        if self.list_multipliers.insert(coordinate, rule).is_some() {
            return Err(CostPolicyConfigurationError);
        }
        Ok(self)
    }
}

impl OperationCostPolicy for StandardOperationCost {
    fn cost(&self, context: OperationCostContext<'_>) -> Result<OperationCost, OperationCostError> {
        let mut total: Option<OperationCost> = None;
        let mut cumulative_multipliers = Vec::with_capacity(context.fields().len());
        for field in context.fields() {
            let inherited: u64 = match field.parent_index() {
                Some(index) => cumulative_multipliers
                    .get(index)
                    .copied()
                    .ok_or(OperationCostError::InvalidStructure)?,
                None => 1,
            };
            let base = self
                .coordinate_weights
                .get(field.coordinate())
                .copied()
                .unwrap_or_else(OperationCost::one);
            let own = self
                .list_multipliers
                .get(field.coordinate())
                .map_or(Ok(1), |rule| list_multiplier(field, rule))?;
            let cumulative = inherited
                .checked_mul(own)
                .ok_or(OperationCostError::Overflow)?;
            cumulative_multipliers.push(cumulative);
            if context.catalog_override().is_none() {
                let cost = base.checked_mul(cumulative)?;
                total = Some(match total {
                    Some(current) => current.checked_add(cost)?,
                    None => cost,
                });
            }
        }
        Ok(context
            .catalog_override()
            .or(total)
            .unwrap_or_else(OperationCost::one))
    }
}

fn list_multiplier(
    field: &CostFieldMetadata,
    rule: &ListMultiplier,
) -> Result<u64, OperationCostError> {
    let Some(argument) = field.argument(&rule.argument) else {
        return Ok(1);
    };
    let Some(value) = argument.bounded_integer() else {
        return match argument.presence() {
            CostArgumentPresence::Absent => Ok(1),
            CostArgumentPresence::Defaulted | CostArgumentPresence::Provided => {
                Err(OperationCostError::InvalidMultiplier)
            }
        };
    };
    if value > rule.maximum {
        return Err(OperationCostError::ListLimitExceeded);
    }
    Ok(value.max(1))
}

fn validate_coordinate(coordinate: &str) -> Result<(), CostPolicyConfigurationError> {
    let Some((parent, field)) = coordinate.split_once('.') else {
        return Err(CostPolicyConfigurationError);
    };
    if coordinate.matches('.').count() != 1 || !graphql_name(parent) || !graphql_name(field) {
        return Err(CostPolicyConfigurationError);
    }
    Ok(())
}

fn graphql_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    matches!(first, b'A'..=b'Z' | b'a'..=b'z' | b'_')
        && bytes.all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::{OperationCost, OperationCostError};

    #[test]
    fn rejects_overflow_and_zero_without_saturation() {
        assert_eq!(OperationCost::new(0), Err(OperationCostError::Zero));
        assert_eq!(
            OperationCost::new(u64::MAX)
                .unwrap()
                .checked_add(OperationCost::one()),
            Err(OperationCostError::Overflow),
        );
    }
}
