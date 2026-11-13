//! Connection-init authorizer replacement and redaction tests.

use chigiri_http::{
    ApplicationMetadata, ConnectionInitContext, ConnectionInitDecision, TransportRequest,
};

use super::super::ConnectionInitAuthorization;
use super::support::{DecisionAuthorizer, ready};
use crate::Endpoint;
use crate::configuration::ConfigurationOrigin;
use crate::subscription::request::test_support::schema;

#[test]
fn baseline_authorizer_accepts() {
    let builder = Endpoint::builder(schema());
    let authorizer = &builder.draft.connection_init_authorizer.value;
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
    let decision = ready(authorizer.authorize(ConnectionInitContext::__new(transport, None)))
        .expect("baseline authorizer must complete");
    assert_eq!(decision, ConnectionInitDecision::Accept);
}

#[test]
fn replacement_is_redacted_and_uses_the_common_leaf() {
    let builder = Endpoint::builder(schema())
        .connection_init_authorizer(DecisionAuthorizer(ConnectionInitDecision::Reject))
        .inject(|components| {
            components
                .subscriptions()
                .connection_init_authorizer(DecisionAuthorizer(ConnectionInitDecision::Accept));
        });
    let authorizer = &builder.draft.connection_init_authorizer.value;
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
    let decision = ready(authorizer.authorize(ConnectionInitContext::__new(transport, None)))
        .expect("authorizer must complete");
    assert_eq!(decision, ConnectionInitDecision::Accept);

    let endpoint = builder.without_context().build().unwrap();
    let snapshot = endpoint.configuration().connection_init_authorization();
    assert_eq!(snapshot.origin(), ConfigurationOrigin::User);
    assert_eq!(
        snapshot.effective(),
        &ConnectionInitAuthorization::ApplicationDefined
    );
    let debug = format!("{:?}", endpoint.configuration());
    assert!(!debug.contains("DecisionAuthorizer"));
}
