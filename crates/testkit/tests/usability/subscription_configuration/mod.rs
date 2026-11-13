//! ADR-0055 numeric contracts through only public application APIs.

mod bounds;
mod graph;
mod numeric;
mod privacy;

use chigiri::{
    ConfigurationConstraint, ConfigurationDiagnostic, ConfigurationOrigin, ConfigurationSlot,
    ConfigurationValue, ConfigurationViolation, Endpoint, EndpointBuildError, SubscriptionLimits,
    SubscriptionProtocolSettings,
};

use super::support::resolvers;

fn build(
    limits: SubscriptionLimits,
    protocol: SubscriptionProtocolSettings,
    inject: bool,
) -> Result<Endpoint<()>, EndpointBuildError> {
    let builder = Endpoint::builder(resolvers());
    let builder = if inject {
        builder.inject(|components| {
            components.subscriptions().limits(limits).protocol(protocol);
        })
    } else {
        builder
            .subscription_limits(limits)
            .subscription_protocol(protocol)
    };
    builder.without_context().build()
}

fn diagnostic(result: Result<Endpoint<()>, EndpointBuildError>) -> ConfigurationDiagnostic {
    let Err(EndpointBuildError::InvalidConfiguration(diagnostic)) = result else {
        panic!("expected an invalid configuration");
    };
    diagnostic
}

fn build_with_parity(
    limits: SubscriptionLimits,
    protocol: SubscriptionProtocolSettings,
) -> Result<Endpoint<()>, EndpointBuildError> {
    let sugar = build(limits, protocol, false);
    let injected = build(limits, protocol, true);
    match (&sugar, injected) {
        (Ok(sugar), Ok(injected)) => assert_eq!(sugar.configuration(), injected.configuration()),
        (
            Err(EndpointBuildError::InvalidConfiguration(sugar)),
            Err(EndpointBuildError::InvalidConfiguration(injected)),
        ) => assert_eq!(*sugar, injected),
        _ => panic!("sugar and injection must agree on the complete final graph"),
    }
    sugar
}

fn assert_range(
    diagnostic: ConfigurationDiagnostic,
    slot: ConfigurationSlot,
    supplied: ConfigurationValue,
    minimum: ConfigurationValue,
    maximum: ConfigurationValue,
    violation: ConfigurationViolation,
) {
    assert_eq!(diagnostic.slot(), slot);
    assert_eq!(diagnostic.origin(), ConfigurationOrigin::User);
    assert_eq!(diagnostic.violation(), violation);
    let expected = ConfigurationConstraint::Range {
        supplied,
        minimum,
        maximum,
    };
    assert_eq!(diagnostic.constraint(), Some(expected));
    assert_eq!(format!("{diagnostic:?}"), diagnostic.to_string());
    assert_eq!(format!("{expected:?}"), expected.to_string());
    for value in [supplied, minimum, maximum] {
        assert!(diagnostic.to_string().contains(&value.to_string()));
    }
}
