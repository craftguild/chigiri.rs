//! Erased application values retain only the scope needed for owned destruction.

use std::any::Any;

use chigiri_core::instrumentation::InstrumentationScope;

pub(super) struct ScopedValue {
    value: Option<Box<dyn Any + Send + Sync>>,
    scope: Option<InstrumentationScope>,
}

impl ScopedValue {
    pub(super) fn new(value: impl Any + Send + Sync) -> Self {
        Self {
            value: Some(Box::new(value)),
            scope: None,
        }
    }

    pub(super) fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value
            .as_deref()
            .expect("erased value is present before destruction")
            .downcast_ref()
    }

    pub(super) fn bind(&mut self, scope: Option<InstrumentationScope>) {
        if scope.is_some() {
            self.scope = scope;
        }
    }
}

impl Drop for ScopedValue {
    fn drop(&mut self) {
        let value = self.value.take();
        match &self.scope {
            Some(scope) => scope.run(|| drop(value)),
            None => drop(value),
        }
    }
}
