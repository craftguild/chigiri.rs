//! Store values by slot ID, validating borrowed inputs and charging shared work before copying.

use std::collections::BTreeMap;
use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{StoreError, Usage};
use crate::ir::{MockIr, ModelId, SlotId};
use crate::value::{ValueBudget, ValueError, ValueValidator};

/// One stored model record, identified by its owning IR and model.
#[derive(Debug)]
pub struct Record {
    scope: Arc<()>,
    pub(super) model: ModelId,
    pub(super) id: Option<Arc<str>>,
    pub(super) order: u64,
    pub(super) values: BTreeMap<SlotId, Value>,
    pub(super) usage: Usage,
}

impl Record {
    pub(crate) fn belongs_to(&self, ir: &MockIr) -> bool {
        Arc::ptr_eq(&self.scope, ir.scope())
    }
    /// Owning storage model.
    pub fn model(&self) -> ModelId {
        self.model
    }
    /// Immutable key tuple.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    /// Internal insertion order, not exposed as a user field.
    pub(crate) fn row(&self) -> u64 {
        self.order
    }
    /// Return the value stored in one slot.
    pub fn get(&self, slot: SlotId) -> Option<&Value> {
        self.values.get(&slot)
    }
    /// Return the stored slots in slot order.
    pub fn values(&self) -> impl ExactSizeIterator<Item = (SlotId, &Value)> {
        self.values.iter().map(|(slot, value)| (*slot, value))
    }
}

pub(super) fn key(ir: &MockIr, model: ModelId) -> Option<SlotId> {
    let model = &ir.models()[model.0];
    Some(model.slots[&ir.graph().fields()[model.key?.0].name])
}

pub(super) struct Basis<'a> {
    pub order: u64,
    pub base: Option<&'a Record>,
    pub generated_id: Option<&'a str>,
}

pub(super) fn build(
    ir: &MockIr,
    model: ModelId,
    raw: &Object,
    basis: Basis<'_>,
    capacity: Usage,
    budget: &mut ValueBudget,
) -> Result<Record, StoreError> {
    build_with_field(ir, model, raw, basis, capacity, budget, &mut None)
}

pub(super) fn build_with_field(
    ir: &MockIr,
    model: ModelId,
    raw: &Object,
    basis: Basis<'_>,
    capacity: Usage,
    budget: &mut ValueBudget,
    field: &mut Option<SlotId>,
) -> Result<Record, StoreError> {
    let Basis {
        order,
        base,
        generated_id,
    } = basis;
    let definition = ir
        .models()
        .get(model.0)
        .ok_or(ValueError("unknown model"))?;
    // Build borrowed slot values and validate required slots before allocating the record entry.
    budget.work.charge(
        base.map_or(0, |row| row.values.len())
            .saturating_add(raw.len())
            .saturating_add(definition.slots.len())
            .saturating_add(definition.required_slots.len()),
    )?;
    let key = key(ir, model);
    let shape = if generated_id.is_some() {
        crate::api::model_values::Shape::Create
    } else if base.is_some() {
        crate::api::model_values::Shape::Update
    } else {
        crate::api::model_values::Shape::Record
    };
    let generated = generated_id.map(Value::from);
    let mut values: BTreeMap<SlotId, &Value> = base.map_or_else(BTreeMap::new, |record| {
        record
            .values
            .iter()
            .map(|(id, value)| (*id, value))
            .collect()
    });
    if let (Some(key), Some(value)) = (key, &generated) {
        values.insert(key, value);
    }
    for (name, value) in raw {
        *field = None;
        let slot = *definition
            .slots
            .get(name)
            .ok_or(ValueError("unknown or computed storage field"))?;
        *field = Some(slot);
        if definition.immutable_slots.contains(&slot) && !shape.policy().include_key {
            return Err(ValueError("model key is immutable"));
        }
        values.insert(slot, value);
    }
    let missing = Value::Null;
    for slot in definition.slots.values() {
        if ir.slots()[slot.0].ty.nullable() {
            values.entry(*slot).or_insert(&missing);
        }
    }
    for slot in &definition.required_slots {
        *field = Some(*slot);
        if !values.contains_key(slot) {
            return Err(ValueError("required storage field missing"));
        }
    }
    let before = (budget.nodes(), budget.retained_bytes());
    *field = None;
    budget.container(1)?;
    for (slot, value) in &values {
        *field = Some(*slot);
        budget.key(&ir.slots()[slot.0].name)?;
        ValueValidator::new(ir, budget).slot_at_depth(*slot, value, 2)?;
    }
    let mut usage = Usage {
        records: 1,
        nodes: budget.nodes() - before.0,
        bytes: budget.retained_bytes() - before.1,
    };
    let indexes = super::index_usage::reserve(ir, model, &values, budget)?;
    usage.nodes += indexes.nodes;
    usage.bytes += indexes.bytes;
    // Validate aggregate candidate capacity before cloning values or building indexes.
    *field = None;
    capacity.replacing(base.map_or(Usage::default(), |record| record.usage), usage)?;
    *field = key;
    let id = super::identity::value(key.and_then(|key| values.get(&key).copied()))?;
    Ok(Record {
        scope: ir.scope().clone(),
        model,
        id,
        order,
        usage,
        values: values
            .into_iter()
            .map(|(slot, value)| (slot, value.clone()))
            .collect(),
    })
}
