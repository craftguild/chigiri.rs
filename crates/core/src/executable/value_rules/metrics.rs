//! Track early termination of value validation.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct VisitMetrics {
    pub(crate) input_fields: usize,
    pub(crate) custom_fields: usize,
}

thread_local! {
    static INPUT_FIELDS: Cell<usize> = const { Cell::new(0) };
    static CUSTOM_FIELDS: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn reset() {
    INPUT_FIELDS.set(0);
    CUSTOM_FIELDS.set(0);
}

pub(super) fn record_input_field() {
    INPUT_FIELDS.with(|count| count.set(count.get() + 1));
}

pub(super) fn record_custom_field() {
    CUSTOM_FIELDS.with(|count| count.set(count.get() + 1));
}

pub(crate) fn get() -> VisitMetrics {
    VisitMetrics {
        input_fields: INPUT_FIELDS.get(),
        custom_fields: CUSTOM_FIELDS.get(),
    }
}
