//! Shared captured ownership survives staging transfer and sticky capture failure.

use super::super::{PublicationError, PublicationLimits, PublicationStore};
use super::support::{fixture, scalar};
use crate::events::SourceLimits;
use crate::store::Store;

#[test]
fn physical_publication_capture_reserves_before_copy_and_failure_is_sticky() {
    let fixture = fixture();
    let limits = SourceLimits::new(1, 2048, 1000, 1024, 512).unwrap();
    let store =
        PublicationStore::with_source_limits(Store::empty(fixture.ir.clone()), 1, limits).unwrap();
    let (_, retention) = store.source_policy();
    let buffer = store
        .capture_owner()
        .buffer(PublicationLimits::new(1, 2048).unwrap())
        .unwrap();
    let baseline = retention.used();
    assert!(baseline > 0 && baseline <= 1024);
    assert_eq!(
        buffer.push(&fixture.ir, fixture.event, "x", &scalar("x".repeat(1024))),
        Err(PublicationError::CaptureLimit)
    );
    assert_eq!(
        buffer.push(&fixture.ir, fixture.event, "x", &scalar("small")),
        Err(PublicationError::InvalidPayload)
    );
    assert!(buffer.prepare(&store.staging()).is_err());
    assert_eq!(retention.used(), 0);
}
