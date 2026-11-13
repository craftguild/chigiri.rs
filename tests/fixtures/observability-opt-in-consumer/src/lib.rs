#![forbid(unsafe_code)]
//! Consumer fixture that does not select an observability backend bridge.

use chigiri::{ErrorObserver, LifecycleObserver};
use chigiri_observability::LifecycleExporter;

/// Fixed backend used by a dependency-free observer through public contracts only.
pub fn observers() -> (impl LifecycleObserver + Clone, impl ErrorObserver + Clone) {
    let (exporter, _, _) = LifecycleExporter::new(8).expect("positive queue capacity");
    (exporter.clone(), exporter)
}
