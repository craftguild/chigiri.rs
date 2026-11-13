//! Validate complete seed records before resolving relations independently of model declaration order.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{MODEL_RECORDS, SeedError, Store, StoreError, Usage, records, relations};
use crate::ir::MockIr;
use crate::value::{ValueBudget, ValueError};

impl Store {
    /// Validate decoded seed data; the JSON boundary is responsible for duplicate-key rejection.
    pub fn seeded(ir: Arc<MockIr>, seed: &Object) -> Result<Self, StoreError> {
        Self::seeded_detailed(ir, seed).map_err(|error| ValueError(error.message))
    }

    pub(super) fn seeded_detailed(ir: Arc<MockIr>, seed: &Object) -> Result<Self, SeedError> {
        let mut store = Self::empty(ir);
        let state = Arc::get_mut(&mut store.current.state).unwrap();
        let mut work = store
            .current
            .ir
            .graph()
            .startup_work()
            .fork_startup()
            .map_err(|error| SeedError::new(error.0))?;
        let mut budget = ValueBudget::with_work(work.clone());
        let mut pending = Vec::new();
        for (name, rows) in seed {
            let ir = &store.current.ir;
            let model = ir
                .graph()
                .type_id(name)
                .and_then(|ty| ir.model_for(ty))
                .ok_or_else(|| SeedError::new("unknown seed model"))?;
            let Value::List(rows) = rows else {
                return Err(SeedError::new("seed model requires an array").at(model, 1));
            };
            if rows.len() > MODEL_RECORDS {
                return Err(SeedError::new("model capacity").at(model, 1));
            }
            for (index, row) in rows.iter().enumerate() {
                let error = |message| SeedError::new(message).at(model, index + 1);
                let Value::Object(row) = row else {
                    return Err(error("seed record requires an object"));
                };
                let order = state.models[model.0].next_order;
                let expanded = super::seed_refs::expand(ir, model, row, seed, &mut budget)
                    .map_err(|e| error(e.0))?;
                let normalized = super::seed_relations::normalize(
                    ir,
                    model,
                    order,
                    &expanded,
                    &mut pending,
                    &mut budget,
                )
                .map_err(|e| error(e.0))?;
                let mut field = None;
                let record = Arc::new(
                    records::build_with_field(
                        ir,
                        model,
                        &normalized,
                        records::Basis {
                            order,
                            base: None,
                            generated_id: None,
                        },
                        state.usage,
                        &mut budget,
                        &mut field,
                    )
                    .map_err(|e| {
                        let mut result = error(e.0);
                        result.field = field.map(|slot| ir.slots()[slot.0].name.clone());
                        if result.field.is_none() && e.0 == "unknown or computed storage field" {
                            // Report only known computed fields by their SDL-derived names; never copy an arbitrary raw key.
                            let node = &ir.graph().types()[ir.models()[model.0].ty.0];
                            result.field = expanded.iter().find_map(|(key, _)| {
                                (!ir.models()[model.0].slots.contains_key(key))
                                    .then(|| {
                                        node.fields
                                            .get(key)
                                            .map(|id| ir.graph().fields()[id.0].name.clone())
                                    })
                                    .flatten()
                            });
                        }
                        result
                    })?,
                );
                if record
                    .id
                    .as_ref()
                    .is_some_and(|id| state.models[model.0].ids.contains_key(id))
                {
                    let mut result = error("duplicate seed ID");
                    result.field = ir.models()[model.0]
                        .key
                        .map(|key| ir.graph().fields()[key.0].name.clone());
                    return Err(result);
                }
                let usage = state
                    .usage
                    .replacing(Usage::default(), record.usage)
                    .map_err(|e| error(e.0))?;
                work.charge(record.usage.nodes).map_err(|e| error(e.0))?;
                relations::replace(ir, state, None, Some(&record), &mut work)
                    .map_err(|e| error(e.0))?;
                let model = &mut state.models[model.0];
                model.next_order = order
                    .checked_add(1)
                    .ok_or_else(|| error("registration order exhausted"))?;
                if let Some(id) = &record.id {
                    model.ids.insert(id.clone(), order);
                }
                model.records.insert(order, record);
                state.usage = usage;
            }
        }
        super::seed_relations::resolve(&store.current.ir, state, &pending, &mut budget)
            .and_then(|_| relations::validate_detailed(&store.current.ir, state, &mut work))
            .map_err(|failure| {
                let mut error = SeedError::new(failure.error.0);
                if let Some(location) = failure.location {
                    error.model = Some(location.model);
                    error.line = location.order as usize + 1;
                    error.field = location.field;
                }
                error
            })?;
        Ok(store)
    }
}
