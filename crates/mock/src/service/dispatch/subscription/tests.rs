//! Preserve one source mapper call while masking all host diagnostic details.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use chigiri::FieldError;

use crate::ir::FieldId;
use crate::luau::ScriptError;

#[test]
fn public_source_diagnostic_is_observer_owned_and_not_client_visible() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let report = move |field: FieldId, error: ScriptError| {
        observed.fetch_add(1, Ordering::SeqCst);
        assert_eq!(field, FieldId(7));
        assert_eq!(error.phase, "source");
        FieldError::public("PRIVATE_PATH [source] PRIVATE_PROVIDER")
            .code("PRIVATE_CODE")
            .extension("private", "PRIVATE_VALUE")
    };
    let mapped = super::source_error(
        &report,
        FieldId(7),
        ScriptError::new("source", "PRIVATE_PROVIDER"),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(mapped.is_internal());
    let retained = mapped
        .internal_source()
        .unwrap()
        .downcast_ref::<FieldError>()
        .unwrap();
    assert!(retained.to_string().contains("PRIVATE_PROVIDER"));
    let visible = mapped.to_graphql_error(Vec::new(), Vec::new());
    assert_eq!(visible.message, "Internal server error");
    assert_eq!(visible.extensions.len(), 1);
    assert_eq!(
        visible.extensions.get("code"),
        Some(&chigiri::Value::from("INTERNAL_SERVER_ERROR")),
    );
    assert!(!format!("{visible:?}").contains("PRIVATE"));
}

#[test]
fn already_internal_source_mapping_retains_the_original_error_without_rewrapping() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let report = move |_: FieldId, _: ScriptError| {
        observed.fetch_add(1, Ordering::SeqCst);
        FieldError::internal(std::io::Error::other("PRIVATE_ORIGINAL"))
    };
    let mapped = super::source_error(&report, FieldId(0), ScriptError::new("source", "private"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let original = mapped
        .internal_source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .expect("the original internal source type remains observable");
    assert_eq!(original.to_string(), "PRIVATE_ORIGINAL");
    assert_eq!(
        mapped.safe_summary(),
        ("Internal server error", Some("INTERNAL_SERVER_ERROR"))
    );
}
