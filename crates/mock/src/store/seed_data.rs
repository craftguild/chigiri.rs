//! Decodes JSONL and bundle JSON into the same owned representation and
//! normalizes only validated initial seed data.

use std::collections::BTreeSet;
use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{MODEL_RECORDS, Store, TOTAL_RECORDS, seed_json};
use crate::ir::{MockIr, ModelId};
use crate::value::ValueBudget;

/// Seed-input location and reason without raw records, IDs, or physical paths.
#[derive(Debug)]
pub struct SeedError {
    /// Known stored-model location.
    pub model: Option<ModelId>,
    /// One-based JSONL line, or record position within a bundled model.
    pub line: usize,
    /// Known stored field name derived from SDL.
    pub field: Option<Arc<str>>,
    /// Stable redacted reason.
    pub message: &'static str,
}

impl SeedError {
    pub(super) fn new(message: &'static str) -> Self {
        Self {
            model: None,
            line: 1,
            field: None,
            message,
        }
    }
    pub(super) fn at(mut self, model: ModelId, line: usize) -> Self {
        self.model = Some(model);
        self.line = line;
        self
    }
}

/// Initial values returned by the decoder, separate from runtime store dumps.
pub struct SeedData {
    values: Object,
}

impl SeedData {
    /// Decode a bundle's strict JSON string with the shared borrowed decoder.
    pub fn json(source: &str) -> Result<Self, SeedError> {
        let values = seed_json::object(source).map_err(|e| SeedError::new(e.0))?;
        Ok(Self { values })
    }

    /// Decode all active JSONL files under one shared budget. Call order does not
    /// change record registration order.
    pub fn jsonl(ir: &MockIr, files: &[(ModelId, &str)]) -> Result<Self, SeedError> {
        Self::check_capacity(ir, files)?;
        let mut seen = BTreeSet::new();
        let mut bytes = 0usize;
        let mut records = 0usize;
        let mut budget = ValueBudget::default();
        budget.container(0).map_err(|e| SeedError::new(e.0))?;
        let mut values = Object::new();
        for &(model, source) in files {
            let error = |message| SeedError::new(message).at(model, 1);
            let definition = ir
                .models()
                .get(model.0)
                .ok_or_else(|| error("unknown seed model"))?;
            if !seen.insert(model) {
                return Err(error("duplicate seed model"));
            }
            if source.len() > crate::limits::SEED_BYTES.saturating_sub(bytes) {
                return Err(error("aggregate JSONL byte limit"));
            }
            bytes += source.len();
            let name = &ir.graph().types()[definition.ty.0].name;
            budget
                .key(name)
                .and_then(|_| budget.container(0))
                .map_err(|e| error(e.0))?;
            let mut rows = Vec::new();
            // `split_terminator` excludes only the final newline; the decoder
            // rejects intermediate blank or whitespace-only lines.
            for (index, line) in source.split_terminator('\n').enumerate() {
                let error = |message| SeedError::new(message).at(model, index + 1);
                if rows.len() == MODEL_RECORDS || records == TOTAL_RECORDS {
                    return Err(error("seed record capacity"));
                }
                let row = seed_json::record(line, &mut budget).map_err(|e| error(e.0))?;
                rows.push(row);
                records += 1;
            }
            values.insert(name.to_string(), Value::List(rows));
        }
        if seen.len() != ir.models().len() {
            return Err(SeedError::new("active seed coverage mismatch"));
        }
        Ok(Self { values })
    }

    /// Check record and value capacity across every active file before type and
    /// storage validation, including data hidden after an invalid line.
    pub(crate) fn check_capacity(ir: &MockIr, files: &[(ModelId, &str)]) -> Result<(), SeedError> {
        super::seed_capacity::check(ir, files)
    }

    /// Discover model files required by explicit references without interpreting
    /// marker-shaped data inside represented JSON scalars.
    pub(crate) fn reference_dependencies(
        ir: &MockIr,
        model: ModelId,
        source: &str,
    ) -> Result<BTreeSet<ModelId>, SeedError> {
        let mut budget = ValueBudget::default();
        let mut found = BTreeSet::new();
        for (index, line) in source.split_terminator('\n').enumerate() {
            let error = |message| SeedError::new(message).at(model, index + 1);
            let row = seed_json::record(line, &mut budget).map_err(|e| error(e.0))?;
            let Value::Object(row) = row else {
                return Err(error("seed record requires an object"));
            };
            found.extend(
                super::seed_ref_dependencies::dependencies(ir, model, &row)
                    .map_err(|e| error(e.0))?,
            );
        }
        Ok(found)
    }

    /// Lower bound for canonical size of the known portion excluding invalid lines.
    /// This still detects expansion of later normalizable lines when no complete
    /// seed exists.
    pub(crate) fn capacity_canonical(
        ir: &MockIr,
        files: &[(ModelId, &str)],
    ) -> Result<String, SeedError> {
        Self::check_capacity(ir, files)?;
        let mut values = Object::new();
        let mut budget = ValueBudget::default();
        for &(model, source) in files {
            let name = &ir.graph().types()[ir.models()[model.0].ty.0].name;
            let mut rows = Vec::new();
            for line in source.split_terminator('\n') {
                if let Ok(row) = seed_json::record(line, &mut budget) {
                    rows.push(row);
                }
            }
            values.insert(name.to_string(), Value::List(rows));
        }
        super::seed_render::render(&values).map_err(SeedError::new)
    }

    /// Validate model types, stored fields, IDs, and relations before returning a
    /// store.
    pub fn store(&self, ir: Arc<MockIr>) -> Result<Store, SeedError> {
        Store::seeded_detailed(ir, &self.values)
    }

    /// Validate a relation-closed component with the same decoder and store rules
    /// as the complete graph.
    /// Excluded models receive empty validation input. This temporary store is
    /// never returned to execution or export.
    pub fn check_component(ir: Arc<MockIr>, files: &[(ModelId, &str)]) -> Result<(), SeedError> {
        let models: BTreeSet<_> = files.iter().map(|(id, _)| *id).collect();
        if models.len() != files.len()
            || models.iter().any(|id| id.0 >= ir.models().len())
            || ir.relations().iter().any(|relation| {
                models.contains(&relation.owner) != models.contains(&relation.target)
            })
        {
            return Err(SeedError::new("seed component coverage mismatch"));
        }
        let mut inputs = files.to_vec();
        for index in 0..ir.models().len() {
            if !models.contains(&ModelId(index)) {
                inputs.push((ModelId(index), ""));
            }
        }
        Self::jsonl(&ir, &inputs)?.store(ir)?;
        Ok(())
    }

    /// Render deterministic JSON that preserves record order and exact numbers
    /// without rebuilding a large JSON DOM.
    pub fn canonical(&self) -> Result<String, SeedError> {
        super::seed_render::render(&self.values).map_err(SeedError::new)
    }

    /// Render seed values after explicit snapshot references have been expanded.
    pub fn resolved_canonical(&self, ir: &MockIr) -> Result<String, SeedError> {
        let mut output = Object::new();
        let mut budget = ValueBudget::default();
        for (name, rows) in &self.values {
            let model = ir
                .graph()
                .type_id(name)
                .and_then(|ty| ir.model_for(ty))
                .ok_or_else(|| SeedError::new("unknown seed model"))?;
            let Value::List(rows) = rows else {
                return Err(SeedError::new("seed model requires an array").at(model, 1));
            };
            let mut expanded = Vec::with_capacity(rows.len());
            for (index, row) in rows.iter().enumerate() {
                let error = |message| SeedError::new(message).at(model, index + 1);
                let Value::Object(row) = row else {
                    return Err(error("seed record requires an object"));
                };
                expanded.push(Value::Object(
                    super::seed_refs::expand(ir, model, row, &self.values, &mut budget)
                        .map_err(|e| error(e.0))?,
                ));
            }
            output.insert(name, Value::List(expanded));
        }
        super::seed_render::render(&output).map_err(SeedError::new)
    }
}
