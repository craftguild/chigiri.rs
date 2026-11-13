#[path = "lifecycle_support.rs"]
pub mod lifecycle_support;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use chigiri_operation::{
    CatalogEntry, CatalogLimits, CatalogRevision, InMemoryOperationCatalog, PersistedOperationId,
};
use chigiri_policy::{OperationCostContext, OperationCostPolicy, UnitFieldCost};
use chigiri_rate::{OperationCost, OperationCostError};

#[test]
fn all_operations_are_revalidated_by_exact_id() {
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("catalog-governance-1").unwrap(),
        vec![
            entry("valid", "query Valid { hello }"),
            entry("syntax", "query Broken {"),
            entry("unknown", "query Unknown { absent }"),
        ],
        CatalogLimits::default(),
    )
    .unwrap();
    let log = lifecycle_support::PhaseLog::default();
    let schema = lifecycle_support::schema(log.clone(), Arc::new(AtomicUsize::new(0)), false);
    let error =
        chigiri_testkit::validate_catalog_impact(&schema, &catalog, &UnitFieldCost).unwrap_err();
    let invalid = error
        .invalid_ids()
        .iter()
        .map(PersistedOperationId::as_str)
        .collect::<Vec<_>>();
    assert_eq!(invalid, ["syntax", "unknown"]);
    assert!(log.snapshot().is_empty());
}

#[test]
fn reports_result_shape_and_cost_deltas_with_exact_ids() {
    let schema = lifecycle_support::schema(
        lifecycle_support::PhaseLog::default(),
        Arc::new(AtomicUsize::new(0)),
        false,
    );
    let baseline = InMemoryOperationCatalog::new(
        CatalogRevision::new("catalog-baseline").unwrap(),
        vec![entry("stable-id", "query Stable { hello }")],
        CatalogLimits::default(),
    )
    .unwrap();
    let current = InMemoryOperationCatalog::new(
        CatalogRevision::new("catalog-current").unwrap(),
        vec![entry("stable-id", "query Stable { renamed: hello }")],
        CatalogLimits::default(),
    )
    .unwrap();
    let baseline =
        chigiri_testkit::validate_catalog_impact(&schema, &baseline, &FixedCost(1)).unwrap();
    let current =
        chigiri_testkit::validate_catalog_impact(&schema, &current, &FixedCost(2)).unwrap();
    let changes = chigiri_testkit::diff_catalog_impact(&baseline, &current);

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id().as_str(), "stable-id");
    assert!(changes[0].identity_changed());
    assert!(changes[0].result_shape_changed());
    assert!(changes[0].complexity_changed());
    assert_eq!(changes[0].baseline_cost().unwrap().get(), 1);
    assert_eq!(changes[0].current_cost().unwrap().get(), 2);
}

#[test]
fn runtime_and_readiness_metadata_match() {
    let schema = lifecycle_support::schema_builder(
        lifecycle_support::PhaseLog::default(),
        Arc::new(AtomicUsize::new(0)),
        false,
    )
    .catalog_revision(Arc::<str>::from("catalog-governance-1"))
    .build()
    .unwrap();
    let readiness = schema.schema_info();

    assert_eq!(readiness.framework_version(), env!("CARGO_PKG_VERSION"));
    assert_eq!(
        readiness.generated_abi(),
        Some(chigiri::GENERATED_ABI_VERSION)
    );
    assert_eq!(
        readiness.schema_digest(),
        chigiri::SchemaDigest::from_bytes([23; 32])
    );
    assert_eq!(readiness.catalog_revision(), Some("catalog-governance-1"));
}

fn entry(id: &str, document: &str) -> CatalogEntry {
    CatalogEntry::new(PersistedOperationId::new(id).unwrap(), document)
}

struct FixedCost(u64);

impl OperationCostPolicy for FixedCost {
    fn cost(
        &self,
        _context: OperationCostContext<'_>,
    ) -> Result<OperationCost, OperationCostError> {
        OperationCost::new(self.0)
    }
}
