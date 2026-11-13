//! Shared subscription settings test support.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use chigiri_http::{
    BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError,
};

use super::super::{SubscriptionLimits, SubscriptionProtocolSettings};
use crate::build_support::EndpointBuildError;
use crate::subscription::request::test_support::schema;
use crate::{ConfigurationOrigin, Endpoint, SettingSnapshot};

pub(super) fn build(
    limits: SubscriptionLimits,
    protocol: SubscriptionProtocolSettings,
) -> Result<Endpoint<()>, EndpointBuildError> {
    Endpoint::builder(schema())
        .subscription_limits(limits)
        .subscription_protocol(protocol)
        .without_context()
        .build()
}

pub(super) fn diagnostic(
    result: Result<Endpoint<()>, EndpointBuildError>,
) -> crate::ConfigurationDiagnostic {
    let Err(EndpointBuildError::InvalidConfiguration(diagnostic)) = result else {
        panic!("expected a subscription configuration diagnostic");
    };
    diagnostic
}

pub(super) fn assert_snapshot<T: Eq + std::fmt::Debug>(
    snapshot: &SettingSnapshot<T>,
    expected: T,
    origin: ConfigurationOrigin,
) {
    assert_eq!(snapshot.baseline(), &expected);
    assert_eq!(snapshot.effective(), &expected);
    assert_eq!(snapshot.origin(), origin);
}

pub(super) struct DecisionAuthorizer(pub(super) ConnectionInitDecision);

impl ConnectionInitAuthorizer for DecisionAuthorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        Box::pin(async move { Ok(self.0) })
    }
}

pub(super) fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test callback must complete immediately"),
    }
}
