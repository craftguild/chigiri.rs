//! Decode each distinct complete output shape without poisoning compatible alternatives.

use chigiri::core::Value;

use super::{Codec, Provenance, VmValue, hint::Hint, scan::Scan};
use crate::ir::{FieldId, MockIr, TypeRef};
use crate::value::ValueError;

pub(crate) struct EventCandidate {
    pub(crate) fields: Vec<FieldId>,
    pub(crate) value: Value,
    pub(crate) provenance: Provenance,
}

impl Codec {
    pub(crate) fn decode_event(
        &mut self,
        ir: &MockIr,
        fields: &[FieldId],
        value: &VmValue,
    ) -> Result<Vec<EventCandidate>, ValueError> {
        self.run(|this| {
            let mut shapes: Vec<(&TypeRef, Vec<FieldId>)> = Vec::new();
            for field in fields {
                let ty = &ir.graph().fields()[field.0].ty;
                if let Some((_, fields)) = shapes.iter_mut().find(|(shape, _)| *shape == ty) {
                    fields.push(*field);
                } else {
                    shapes.push((ty, vec![*field]));
                }
            }
            let raw = this.scope.value(value)?;
            let mut candidates = Vec::new();
            for (ty, fields) in shapes {
                let mut scan = Scan::default();
                let decoded = this.decode_value(ir, Hint::typed(ty), raw, 1, &mut scan);
                // Shape mismatch is an alternative, never a reason to clear a
                // resource failure or reset the shared conversion/work budget.
                if this.budget.work.failure().is_some() {
                    return Err(ValueError("event conversion resource limit"));
                }
                if let Ok(value) = decoded {
                    candidates.push(EventCandidate {
                        fields,
                        value,
                        provenance: scan.provenance,
                    });
                }
            }
            if candidates.is_empty() {
                Err(ValueError("event matches no output shape"))
            } else {
                Ok(candidates)
            }
        })
    }
}
