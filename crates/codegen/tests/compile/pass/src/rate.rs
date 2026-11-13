//! Verify equivalent facade rate configuration through convenience and typed APIs.

use std::time::Duration;

use chigiri::rate::{OperationQuotaExtractor, TransportQuotaExtractor};

use super::{MutationFields, QueryFields, TeamFields, Types, UserFields, generated};

struct TransportKey;

impl TransportQuotaExtractor for TransportKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        key().map_err(chigiri::spi::AdmissionError::new)
    }
}

struct OperationKey;

impl OperationQuotaExtractor<()> for OperationKey {
    fn extract(
        &self,
        _: &chigiri::OperationContext<'_, ()>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::GraphQlError> {
        key().map_err(|_| chigiri::GraphQlError::new("quota key failed"))
    }
}

#[test]
fn limit_07_and_10_defaults_and_convenience_injection_share_the_same_stage_snapshot() {
    let stage_a = transport_stage();
    assert_eq!(
        stage_a.failure_policy(),
        chigiri::rate::ProviderFailurePolicy::FailClosed
    );
    assert_eq!(stage_a.provider_timeout(), Duration::from_millis(500));
    let sugar = chigiri::Endpoint::builder(resolvers())
        .rate_limit(stage_a)
        .without_context()
        .build()
        .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.rate_limit().stage_a(transport_stage());
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        sugar.configuration().stage_a_rate_limit(),
        injected.configuration().stage_a_rate_limit()
    );

    let both = chigiri::Endpoint::builder(resolvers())
        .rate_limit(transport_stage())
        .inject(|components| {
            components.rate_limit().stage_b(operation_stage());
        })
        .without_context()
        .build()
        .unwrap();
    assert_eq!(
        both.configuration().rate_limit_coverage(),
        chigiri::RateLimitCoverage::Both
    );
    assert_eq!(
        both.configuration()
            .stage_b_rate_limit()
            .unwrap()
            .provider_timeout(),
        Duration::from_millis(500)
    );
}

fn transport_stage()
-> chigiri::rate::TransportRateStage<chigiri::rate::StageARateLimit<TransportKey>> {
    chigiri::rate::transport(TransportKey)
        .token_bucket()
        .capacity(120)
        .unwrap()
        .refill(60, Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap()
}

fn operation_stage()
-> chigiri::rate::OperationRateStage<chigiri::rate::OperationRateLimit<OperationKey>> {
    chigiri::rate::operation(OperationKey)
        .sliding_window_counter()
        .limit(10_000)
        .unwrap()
        .window(Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap()
}

fn resolvers() -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_user(UserFields)
        .with_team(TeamFields)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .build()
}

fn key() -> Result<chigiri::rate::QuotaKey, chigiri::rate::QuotaKeyError> {
    chigiri::rate::QuotaKey::new(b"pass".to_vec(), chigiri::rate::QuotaSubject::Missing, None)
}
