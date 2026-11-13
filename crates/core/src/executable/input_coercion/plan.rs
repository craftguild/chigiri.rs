//! Build each input object's fixed field plan once per request.

use std::collections::BTreeMap;

use crate::language::ast::TypeRefKind;
use crate::schema::SemanticType;

use super::engine::Engine;

pub(super) struct InputObjectPlan {
    positions: BTreeMap<String, usize>,
    active_positions: Vec<usize>,
}

impl InputObjectPlan {
    fn build(named: &SemanticType) -> Self {
        record_plan_build(named.input_fields().len());
        let mut positions = BTreeMap::new();
        let mut active_positions = Vec::new();
        for (position, field) in named.input_fields().iter().enumerate() {
            positions.insert(field.name.value.clone(), position);
            let required =
                matches!(field.ty.kind, TypeRefKind::NonNull(_)) && field.default_value.is_none();
            if required || field.default_value.is_some() {
                active_positions.push(position);
            }
        }
        Self {
            positions,
            active_positions,
        }
    }
}

impl Engine<'_> {
    pub(super) fn input_field_position(
        &mut self,
        named: &SemanticType,
        field_name: &str,
    ) -> Option<usize> {
        self.input_plan(named).positions.get(field_name).copied()
    }

    pub(super) fn active_input_positions(&mut self, named: &SemanticType) -> Vec<usize> {
        self.input_plan(named).active_positions.clone()
    }

    fn input_plan(&mut self, named: &SemanticType) -> &InputObjectPlan {
        self.input_plans
            .entry(named.name().to_owned())
            .or_insert_with(|| InputObjectPlan::build(named))
    }
}

pub(super) fn record_occurrence_field() {
    #[cfg(test)]
    METRICS.with(|metrics| metrics.borrow_mut().occurrence_fields += 1);
}

fn record_plan_build(fields: usize) {
    #[cfg(not(test))]
    let _ = fields;
    #[cfg(test)]
    METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.plan_builds += 1;
        metrics.plan_fields += fields;
    });
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PlanMetrics {
    pub(super) plan_builds: usize,
    pub(super) plan_fields: usize,
    pub(super) occurrence_fields: usize,
}

#[cfg(test)]
thread_local! {
    static METRICS: std::cell::RefCell<PlanMetrics> = const {
        std::cell::RefCell::new(PlanMetrics {
            plan_builds: 0,
            plan_fields: 0,
            occurrence_fields: 0,
        })
    };
}

#[cfg(test)]
pub(super) fn reset_metrics() {
    METRICS.with(|metrics| *metrics.borrow_mut() = PlanMetrics::default());
}

#[cfg(test)]
pub(super) fn metrics() -> PlanMetrics {
    METRICS.with(|metrics| *metrics.borrow())
}
