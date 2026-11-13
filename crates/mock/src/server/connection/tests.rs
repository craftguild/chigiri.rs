//! Native cutoff retains the one application deadline after SDK-first stop.

use chigiri_mock::cancellation::Cancellation;

use crate::server::shutdown::Shutdown;

#[test]
fn connection_cutoff_keeps_the_original_absolute_deadline() {
    let shutdown = Shutdown::new(Cancellation::default(), |_| {});
    let original = shutdown.begin();
    assert_eq!(shutdown.begin(), original);
    assert_eq!(
        super::http_deadline(original),
        original - super::Duration::from_millis(500)
    );
    assert_eq!(shutdown.begin_native(), original);
    assert_eq!(
        super::http_deadline(shutdown.begin_native()),
        super::http_deadline(original)
    );
}
