//! Validate live row identity and exact sealed captured slots through distinct authority paths.

use super::Validator;
use crate::ir::ModelId;
use crate::output::ObjectData;
use crate::value::{ValueError, ValueValidator};
use chigiri::core::Object;
use std::sync::Arc;

impl Validator<'_> {
    pub(super) fn captured_record(
        &mut self,
        model: ModelId,
        object: &Object,
        origin: Arc<crate::output::CapturedRecord>,
        depth: usize,
    ) -> Result<ObjectData, ValueError> {
        let ir = self.snapshot.ir();
        if !origin.belongs_to(ir) || origin.model() != model {
            return Err(ValueError("foreign captured record provenance"));
        }
        self.validate_record_fields(model, object, depth)?;
        let supplied = object.len() - usize::from(object.get("__typename").is_some());
        if supplied != origin.values().len()
            || !origin.values().all(|(slot, value)| {
                object
                    .get(&ir.slots()[slot.0].name)
                    .is_some_and(|actual| crate::value::equal(value, actual))
            })
        {
            return Err(ValueError("captured record modified"));
        }
        Ok(ObjectData::CapturedRecord(origin))
    }

    pub(super) fn record(
        &mut self,
        model: ModelId,
        object: &Object,
        depth: usize,
    ) -> Result<ObjectData, ValueError> {
        let ir = self.snapshot.ir();
        let definition = &ir.models()[model.0];
        for (name, value) in object {
            if name == "__typename" {
                continue;
            }
            self.budget.key(name)?;
            let slot = *definition
                .slots
                .get(name)
                .ok_or(ValueError("unknown or computed output record field"))?;
            ValueValidator::new(ir, self.budget).slot_at_depth(slot, value, depth + 1)?;
        }
        if let Some(record) = self.snapshot.matching_record(model, object, self.budget)? {
            return Ok(ObjectData::Model(record));
        }
        // Stored values without identity or reference keys may still be returned from root or virtual fields.
        // Keyed records and relation owners require an exact snapshot match, preventing forged or modified copies.
        if definition.key.is_none()
            && definition.immutable_slots.is_empty()
            && !ir
                .relations()
                .iter()
                .any(|relation| relation.owner == model)
        {
            return self.dto(definition.ty, object, depth);
        }
        Err(ValueError("output record absent or modified"))
    }

    pub(super) fn provenance_record(
        &mut self,
        model: ModelId,
        object: &Object,
        origin: Arc<crate::store::Record>,
        depth: usize,
    ) -> Result<ObjectData, ValueError> {
        if !origin.belongs_to(self.snapshot.ir()) || origin.model() != model {
            return Err(ValueError("foreign output record provenance"));
        }
        let Some(current) = self.snapshot.row(model, origin.row()) else {
            return Err(ValueError("stale output record provenance"));
        };
        if !Arc::ptr_eq(current, &origin) {
            return Err(ValueError("stale output record provenance"));
        }
        self.validate_record_fields(model, object, depth)?;
        let supplied = object.len() - usize::from(object.get("__typename").is_some());
        if supplied != origin.values().len() {
            return Err(ValueError("output record modified"));
        }
        let ir = self.snapshot.ir();
        if !origin.values().all(|(slot, value)| {
            object
                .get(&ir.slots()[slot.0].name)
                .is_some_and(|actual| crate::value::equal(value, actual))
        }) {
            return Err(ValueError("output record modified"));
        }
        Ok(ObjectData::Model(origin))
    }

    pub(super) fn unprovenanced_record(
        &mut self,
        model: ModelId,
        object: &Object,
        depth: usize,
    ) -> Result<ObjectData, ValueError> {
        let ir = self.snapshot.ir();
        let definition = &ir.models()[model.0];
        if definition.key.is_none()
            && definition.immutable_slots.is_empty()
            && !ir
                .relations()
                .iter()
                .any(|relation| relation.owner == model)
        {
            return self.dto(definition.ty, object, depth);
        }
        Err(ValueError("output record provenance required"))
    }

    fn validate_record_fields(
        &mut self,
        model: ModelId,
        object: &Object,
        depth: usize,
    ) -> Result<(), ValueError> {
        let ir = self.snapshot.ir();
        let definition = &ir.models()[model.0];
        for (name, value) in object {
            if name == "__typename" {
                continue;
            }
            self.budget.key(name)?;
            let slot = *definition
                .slots
                .get(name)
                .ok_or(ValueError("unknown or computed output record field"))?;
            ValueValidator::new(ir, self.budget).slot_at_depth(slot, value, depth + 1)?;
        }
        Ok(())
    }
}
