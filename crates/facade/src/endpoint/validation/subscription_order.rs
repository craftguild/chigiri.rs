//! Complete pure configuration validation precedes application initialization.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_runtime::{RequestId, RequestIdFactory, RequestIdFactoryError};

use crate::subscription::request::test_support::schema;
use crate::{
    ConfigurationOrigin, ConfigurationSlot, ConfigurationViolation, Endpoint, EndpointBuildError,
    SubscriptionLimits,
};

struct CountingRequestIds {
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl RequestIdFactory for CountingRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(RequestIdFactoryError::new(std::io::Error::other(
                "private identity initialization source",
            )))
        } else {
            Ok(RequestId::new("request-initialized-once").unwrap())
        }
    }
}

#[test]
fn invalid_subscription_graph_never_invokes_a_configured_identity_factory() {
    let cases = [
        (
            SubscriptionLimits {
                max_active_subscriptions: 0,
                ..SubscriptionLimits::default()
            },
            ConfigurationSlot::ActiveSubscriptions,
            ConfigurationViolation::OutsideHardBound,
        ),
        (
            SubscriptionLimits {
                max_retained_metadata_bytes: 64 * 1024 + 1,
                ..SubscriptionLimits::default()
            },
            ConfigurationSlot::RetainedSubscriberMetadata,
            ConfigurationViolation::OutsideHardBound,
        ),
        (
            SubscriptionLimits {
                max_encoded_event_bytes: 1024 * 1024,
                ..SubscriptionLimits::default()
            },
            ConfigurationSlot::EncodedSubscriptionEventBytes,
            ConfigurationViolation::InconsistentLimits,
        ),
    ];
    for (limits, slot, violation) in cases {
        for fail in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let result = Endpoint::builder(schema())
                .subscription_limits(limits)
                .inject(|components| {
                    components
                        .observability()
                        .request_id_factory(CountingRequestIds {
                            calls: Arc::clone(&calls),
                            fail,
                        });
                })
                .without_context()
                .build();
            let Err(EndpointBuildError::InvalidConfiguration(diagnostic)) = result else {
                panic!(
                    "invalid subscription graph must take precedence over identity initialization"
                );
            };
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(diagnostic.slot(), slot);
            assert_eq!(diagnostic.origin(), ConfigurationOrigin::User);
            assert_eq!(diagnostic.violation(), violation);
            assert!(!format!("{diagnostic:?}").contains("private identity"));
        }
    }
}

#[test]
fn valid_subscription_graph_preserves_once_only_identity_initialization() {
    for fail in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let result = Endpoint::builder(schema())
            .inject(|components| {
                components
                    .observability()
                    .request_id_factory(CountingRequestIds {
                        calls: Arc::clone(&calls),
                        fail,
                    });
            })
            .without_context()
            .build();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if fail {
            assert!(matches!(
                result,
                Err(EndpointBuildError::IdentityInitialization)
            ));
        } else {
            let endpoint = result.unwrap();
            let _ = endpoint.request_telemetry(None);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}
