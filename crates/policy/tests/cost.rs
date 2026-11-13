#[path = "support.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_policy::{
    OperationCostContext, OperationCostPolicy, OperationQuotaExtractor, OperationRateLimit,
    ProviderFailurePolicy, StandardOperationCost,
};
use chigiri_rate::{
    MonotonicTick, OperationCost, OperationCostError, QuotaKey, QuotaSubject, TokenBucketPolicy,
};
use chigiri_rate_memory::{InMemoryRateLimitConfig, InMemoryTokenBucket, ManualClock};
use chigiri_runtime::{
    ExecutionLimits, GraphQlError, GraphQlRequest, Object, OperationContext, Value,
};

struct FixedCost(Result<OperationCost, OperationCostError>);

impl OperationCostPolicy for FixedCost {
    fn cost(&self, _: OperationCostContext<'_>) -> Result<OperationCost, OperationCostError> {
        self.0
    }
}

struct FixedKey;

impl OperationQuotaExtractor<support::TestContext> for FixedKey {
    fn extract(
        &self,
        _: &OperationContext<'_, support::TestContext>,
    ) -> Result<QuotaKey, GraphQlError> {
        Ok(QuotaKey::new(b"budget".to_vec(), QuotaSubject::Missing, None).unwrap())
    }
}

fn plugin(cost: FixedCost, max: u64) -> OperationRateLimit<FixedKey> {
    let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)));
    let limiter = InMemoryTokenBucket::with_clock(memory_config(), clock);
    OperationRateLimit::new(
        FixedKey,
        cost,
        limiter,
        TokenBucketPolicy::new(100, 100, Duration::from_secs(1))
            .unwrap()
            .into(),
        ProviderFailurePolicy::FailClosed,
        OperationCost::new(max).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn budget_01_structural_limit_and_operation_cost_are_independently_rejected() {
    let structural_calls = Arc::new(AtomicUsize::new(0));
    let mut limits = ExecutionLimits::production();
    limits.max_expanded_fields = 1;
    let structural = support::builder(Arc::clone(&structural_calls))
        .limits(limits)
        .plugin(plugin(FixedCost(Ok(OperationCost::one())), 10))
        .build()
        .unwrap();
    let response = structural
        .execute(GraphQlRequest::new("{ hello other }"), support::TestContext)
        .await;
    assert_eq!(code(&response), Some("GRAPHQL_VALIDATION_FAILED"));
    assert!(response.errors[0].message.contains("expanded field count"));
    assert_eq!(structural_calls.load(Ordering::SeqCst), 0);

    let cost_calls = Arc::new(AtomicUsize::new(0));
    let cost_limited = support::builder(Arc::clone(&cost_calls))
        .plugin(plugin(FixedCost(Ok(OperationCost::new(2).unwrap())), 1))
        .build()
        .unwrap();
    let response = cost_limited
        .execute(GraphQlRequest::new("{ hello }"), support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(cost_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn budget_02_checked_overflow_is_rejected_before_execution_starts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let schema = support::builder(Arc::clone(&calls))
        .plugin(plugin(FixedCost(Err(OperationCostError::Overflow)), 10))
        .build()
        .unwrap();
    let response = schema
        .execute(GraphQlRequest::new("{ hello }"), support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn base_cost_is_applied_before_coordinate_and_bounded_integer_multipliers() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_coordinate_weight("Query.items", OperationCost::new(2).unwrap())
        .unwrap()
        .with_list_multiplier("Query.items", "first", 10)
        .unwrap();
    let limiter = InMemoryTokenBucket::with_clock(
        memory_config(),
        Arc::new(ManualClock::new(MonotonicTick::from_nanos(0))),
    );
    let plugin = OperationRateLimit::new(
        FixedKey,
        policy,
        limiter,
        TokenBucketPolicy::new(100, 100, Duration::from_secs(1))
            .unwrap()
            .into(),
        ProviderFailurePolicy::FailClosed,
        OperationCost::new(5).unwrap(),
    )
    .unwrap();
    let schema = support::builder(Arc::clone(&calls))
        .plugin(plugin)
        .build()
        .unwrap();
    let response = schema
        .execute(
            GraphQlRequest::new("{ items(first: 3) }"),
            support::TestContext,
        )
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn catalog_override_is_preferred_to_standard_coordinate_cost() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_coordinate_weight("Query.items", OperationCost::new(20).unwrap())
        .unwrap();
    let limiter = InMemoryTokenBucket::with_clock(
        memory_config(),
        Arc::new(ManualClock::new(MonotonicTick::from_nanos(0))),
    );
    let plugin = OperationRateLimit::new(
        FixedKey,
        policy,
        limiter,
        TokenBucketPolicy::new(100, 100, Duration::from_secs(1))
            .unwrap()
            .into(),
        ProviderFailurePolicy::FailClosed,
        OperationCost::one(),
    )
    .unwrap();
    let schema = support::builder(Arc::clone(&calls))
        .plugin(plugin)
        .build()
        .unwrap();
    let response = schema
        .execute(
            GraphQlRequest::new("{ items(first: 3) }").with_catalog_cost_hint(1),
            support::TestContext,
        )
        .await;
    assert!(response.errors.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn catalog_hint_does_not_override_literal_list_costs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", 10)
        .unwrap();
    let request = GraphQlRequest::new("{ groups(first: 11) { items { expensive } } }")
        .with_catalog_cost_hint(1);
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, 100))
        .build()
        .unwrap()
        .execute(request, support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn catalog_overrides_apply_without_exceeding_variable_list_limits() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", 10)
        .unwrap();
    let mut variables = Object::new();
    variables.insert("first".to_owned(), Value::Int(11));
    let request = GraphQlRequest::new(
        "query($first: Int!) { groups(first: $first) { items { expensive } } }",
    )
    .with_variables(variables)
    .with_catalog_cost_hint(1);
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, 100))
        .build()
        .unwrap()
        .execute(request, support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn catalog_hint_does_not_override_nested_list_costs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", u64::MAX)
        .unwrap()
        .with_list_multiplier("Group.items", "first", u64::MAX)
        .unwrap()
        .with_list_multiplier("Item.children", "first", u64::MAX)
        .unwrap();
    let request = GraphQlRequest::new(
        "{ groups(first: 2147483647) { items(first: 2147483647) { \
         children(first: 2147483647) { expensive } } } }",
    )
    .with_catalog_cost_hint(1);
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, u64::MAX))
        .build()
        .unwrap()
        .execute(request, support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn list_multipliers_propagate_to_child_selections_through_aliases_fragments_and_variables() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", 10)
        .unwrap()
        .with_list_multiplier("Group.items", "first", 10)
        .unwrap()
        .with_coordinate_weight("Item.expensive", OperationCost::new(2).unwrap())
        .unwrap();
    let mut variables = Object::new();
    variables.insert("outer".to_owned(), Value::Int(2));
    let request = GraphQlRequest::new(
        "query($outer: Int!, $inner: Int = 3) { \
         alias: groups(first: $outer) { ...Rows } } \
         fragment Rows on Group { items(first: $inner) { expensive } }",
    )
    .with_variables(variables);
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, 19))
        .build()
        .unwrap()
        .execute(request, support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn schema_default_list_sizes_propagate_to_child_selections() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", 10)
        .unwrap()
        .with_list_multiplier("Group.items", "first", 10)
        .unwrap();
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, 9))
        .build()
        .unwrap()
        .execute(
            GraphQlRequest::new("{ groups { items(first: 2) { expensive } } }"),
            support::TestContext,
        )
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn oversized_lists_are_rejected_before_clamping_or_resolver_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", 10)
        .unwrap();
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, 100))
        .build()
        .unwrap()
        .execute(
            GraphQlRequest::new("{ groups(first: 11) { items { expensive } } }"),
            support::TestContext,
        )
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn nested_list_multiplier_overflow_is_rejected_before_resolver_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = StandardOperationCost::new()
        .with_list_multiplier("Query.groups", "first", u64::MAX)
        .unwrap()
        .with_list_multiplier("Group.items", "first", u64::MAX)
        .unwrap()
        .with_list_multiplier("Item.children", "first", u64::MAX)
        .unwrap();
    let request = GraphQlRequest::new(
        "{ groups(first: 2147483647) { items(first: 2147483647) { \
         children(first: 2147483647) { expensive } } } }",
    );
    let response = support::builder(Arc::clone(&calls))
        .plugin(standard_plugin(policy, u64::MAX))
        .build()
        .unwrap()
        .execute(request, support::TestContext)
        .await;
    assert_eq!(code(&response), Some("OPERATION_COST_EXCEEDED"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn standard_plugin(policy: StandardOperationCost, maximum: u64) -> OperationRateLimit<FixedKey> {
    OperationRateLimit::new(
        FixedKey,
        policy,
        InMemoryTokenBucket::with_clock(
            memory_config(),
            Arc::new(ManualClock::new(MonotonicTick::from_nanos(0))),
        ),
        TokenBucketPolicy::new(u64::MAX, u64::MAX, Duration::from_secs(1))
            .unwrap()
            .into(),
        ProviderFailurePolicy::FailClosed,
        OperationCost::new(maximum).unwrap(),
    )
    .unwrap()
}

fn code(response: &chigiri_runtime::GraphQlResponse) -> Option<&str> {
    response.errors.first()?.extensions.get("code")?.as_str()
}

fn memory_config() -> InMemoryRateLimitConfig {
    InMemoryRateLimitConfig::new(4, Duration::from_secs(60)).unwrap()
}
