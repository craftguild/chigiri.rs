use chigiri_http::{
    ApplicationMetadata, DecodedRequestHook, MetadataField, OperationSourceResolver, Rejection,
    TransportAdmission, TransportRequest,
};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn transport_contract_is_object_safe_without_an_associated_body_type() {
    let admission: Option<&dyn TransportAdmission> = None;
    let decoded: Option<&dyn DecodedRequestHook> = None;
    let source: Option<&dyn OperationSourceResolver> = None;
    assert!(admission.is_none() && decoded.is_none() && source.is_none());
    assert_send_sync::<Rejection>();
    assert_send_sync::<ApplicationMetadata>();
    assert_eq!(
        std::mem::size_of::<TransportRequest<'_>>(),
        std::mem::size_of::<TransportRequest<'_>>()
    );
}

#[test]
fn application_metadata_is_bounded_by_entry_count_and_value_size() {
    let field = MetadataField::new("tenant-hash", vec![7; 256]).unwrap();
    assert!(ApplicationMetadata::new(vec![field; 16]).is_err());
    assert!(MetadataField::new("tenant", vec![7; 257]).is_err());
}
