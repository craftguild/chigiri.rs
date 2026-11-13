use chigiri_operation::{
    CatalogEntry, CatalogLimits, CatalogRevision, InMemoryOperationCatalog, OperationCatalog,
    OperationRegistry, PersistedOperationId,
};

#[test]
fn registry_and_catalog_have_distinct_object_safe_contracts() {
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("revision-1").unwrap(),
        vec![CatalogEntry::new(
            PersistedOperationId::new("operation-1").unwrap(),
            "query One { value }",
        )],
        CatalogLimits::default(),
    )
    .unwrap();
    let registry: &dyn OperationRegistry = &catalog;
    let enumerable: &dyn OperationCatalog = &catalog;
    assert_eq!(registry.revision(), Some("revision-1"));
    assert_eq!(enumerable.entries().len(), 1);
}
