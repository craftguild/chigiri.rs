//! Validate a publication payload against the current private mutation candidate.

use std::sync::Arc;

use super::{Snapshot, Transaction};
use crate::ir::FieldId;
use crate::output::{OutputError, OutputValue};

impl Transaction {
    pub(crate) fn publication_event(
        &mut self,
        candidates: &[crate::codec::EventCandidate],
    ) -> Result<(Vec<FieldId>, OutputValue), OutputError> {
        if self.failed {
            return Err(crate::value::ValueError("output failure prevents execution").into());
        }
        let result = (|| {
            let snapshot = self.publication_snapshot()?;
            let mut fields = Vec::new();
            let mut captured = None;
            for candidate in candidates {
                for field in &candidate.fields {
                    let result = crate::output::validate_provenance_detailed(
                        &snapshot,
                        *field,
                        &candidate.value,
                        &candidate.provenance,
                        &mut self.values,
                    );
                    match result {
                        Ok(output) => {
                            fields.push(*field);
                            if captured.is_none() {
                                captured = Some(output);
                            }
                        }
                        Err(error)
                            if error.is_resource_limit() || self.work.failure().is_some() =>
                        {
                            return Err(error);
                        }
                        Err(_) => {}
                    }
                }
            }
            captured.map(|output| (fields, output)).ok_or_else(|| {
                crate::value::ValueError("event matches no subscription output").into()
            })
        })();
        self.failed |= result.is_err();
        result
    }

    fn publication_snapshot(&mut self) -> Result<Snapshot, OutputError> {
        self.work.charge(
            1 + self.state.models.len() + self.state.relations.len() + self.state.keys.len(),
        )?;
        for model in &self.state.models {
            self.work.charge(model.records.len() + model.ids.len())?;
        }
        let snapshot = Snapshot {
            ir: self.base.ir.clone(),
            state: Arc::new(self.state.clone()),
        };
        self.work.charge(0)?;
        Ok(snapshot)
    }
}
