//! Numeric exceptions never disclose arbitrary text or another setting's value.

use chigiri::{
    BoxFuture, ConfigurationSlot, ConnectionInitAuthorizer, ConnectionInitContext,
    ConnectionInitDecision, ConnectionInitError, Endpoint, EndpointBuildError, SubscriptionLimits,
};

use super::{diagnostic, resolvers};

const SECRET: &str = "PRIVATE_credentials_headers_document_reason_provider";

#[derive(Debug)]
struct PrivateProvider(&'static str);

impl ConnectionInitAuthorizer for PrivateProvider {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        let _ = self.0;
        panic!("configuration validation must not invoke the provider");
    }
}

#[test]
fn allowlisted_evidence_does_not_format_reasons_or_provider_state() {
    let error = Endpoint::builder(resolvers())
        .subscription_limits(SubscriptionLimits {
            max_active_subscriptions: 1_025,
            ..SubscriptionLimits::default()
        })
        .inject(|c| {
            c.execution().dangerously_enable_introspection(SECRET);
            c.subscriptions()
                .connection_init_authorizer(PrivateProvider(SECRET));
        })
        .without_context()
        .build()
        .err()
        .unwrap();
    let EndpointBuildError::InvalidConfiguration(details) = &error else {
        panic!("expected numeric rejection");
    };
    assert!(details.constraint().is_some());
    for output in [
        format!("{error:?}"),
        error.to_string(),
        format!("{details:?}"),
        details.to_string(),
    ] {
        assert!(output.contains("1025 count"));
        assert!(!output.contains(SECRET));
        assert!(!output.contains("PrivateProvider"));
    }
}

#[test]
fn non_subscription_slots_keep_no_value_evidence() {
    let details = diagnostic(
        Endpoint::builder(resolvers())
            .body_limit(1_073_741_825)
            .inject(|c| {
                c.execution().dangerously_enable_introspection(SECRET);
                c.subscriptions()
                    .connection_init_authorizer(PrivateProvider(SECRET));
            })
            .without_context()
            .build(),
    );
    assert_eq!(details.slot(), ConfigurationSlot::RequestBodyByteLimit);
    assert_eq!(details.constraint(), None);
    for output in [format!("{details:?}"), details.to_string()] {
        assert!(!output.contains("1073741825"));
        assert!(!output.contains(SECRET));
        assert!(!output.contains("constraint"));
    }
}
