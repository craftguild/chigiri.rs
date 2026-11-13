#[path = "support.rs"]
mod support;

use chigiri_operation::{
    CatalogEntry, CatalogLimits, CatalogRevision, InMemoryOperationCatalog, PersistedOperationId,
    validate_catalog,
};
use chigiri_runtime::OperationName;

fn entry(id: &str, document: &str) -> CatalogEntry {
    CatalogEntry::new(PersistedOperationId::new(id).unwrap(), document)
}

#[test]
fn op_03_catalog_rejects_duplicates_invalid_entries_and_unknown_schema_incompatibility() {
    let revision = CatalogRevision::new("catalog-1").unwrap();
    assert!(
        InMemoryOperationCatalog::new(
            revision.clone(),
            vec![entry("same", "{ value }"), entry("same", "{ other }")],
            CatalogLimits::default(),
        )
        .is_err()
    );
    let catalog = InMemoryOperationCatalog::new(
        revision,
        vec![
            entry("valid", "query Valid { value }")
                .with_operation_name(OperationName::new("Valid").unwrap()),
            entry("invalid", "query Broken {"),
            entry("unknown", "query Unknown { absent }"),
        ],
        CatalogLimits::default(),
    )
    .unwrap();
    let error = validate_catalog(&support::schema(), &catalog).unwrap_err();
    let ids = error
        .invalid_ids()
        .iter()
        .map(PersistedOperationId::as_str)
        .collect::<Vec<_>>();
    assert_eq!(ids, ["invalid", "unknown"]);
}
