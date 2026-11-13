# ADR-0022: Separate the RateLimiter SPI into a runtime-independent crate

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-30
- Decision owners: Pythia maintainers
- Depends on: ADR-0011, ADR-0017, ADR-0019
- Coordinates: ADR-0023
- Compatibility: pre-release breaking change

## 1. Context

ADR-0019 made four official algorithms and third-party providers
interchangeable through one `RateLimiter`. The SPI, quota keys, policies,
decisions, and official in-memory implementations currently coexist in
`pythia_policy`. Because Stage A, Stage B, and runtime plugins make
`pythia_policy` depend on `pythia_http` and `pythia_runtime`, a user implementing
a Redis or gateway provider in a separate crate must also accept GraphQL
execution dependencies.

The minimum provider contract, Pythia operation-cost and endpoint composition,
and official storage implementations need separate crate boundaries. Rate
phase ordering, failure policy, and atomicity do not change.

## 2. Decision

### 2.1 New crate and dependency direction

Add `pythia_rate` to the workspace. It is std-only and has zero normal
dependencies.

```text
pythia_rate
  <- pythia_rate_memory
  <- pythia_policy

pythia_rate + pythia_rate_memory + pythia_policy
  <- pythia façade
```

`pythia_rate` does not depend on HTTP, the GraphQL runtime, an async runtime,
Serde, or official storage and algorithm implementations. Rate types do not
move into `pythia_core`.

### 2.2 Public SPI

Types required to implement a provider move into `pythia_rate`:

- `OperationCost` / `OperationCostError`
- `QuotaKey` / `QuotaKeyError` / `QuotaSubject`
- `PolicyRevision`
- `TokenBucketPolicy`, `FixedWindowPolicy`, `SlidingWindowPolicy`,
  `LeakyBucketPolicy`, and the closed `RatePolicy`
- `RateLimitRequest`
- `RateLimitDecision`, `RateLimitSnapshot`, and `RateLimitRejectReason`
- `RateLimitProviderError`
- `RateLimitFuture`
- `RateLimiter`

Their conceptual signature is fixed as follows:

```rust,ignore
pub type RateLimitFuture<'a> = std::pin::Pin<Box<
    dyn std::future::Future<
            Output = Result<RateLimitDecision, RateLimitProviderError>,
        > + Send
        + 'a,
>>;

pub trait RateLimiter: Send + Sync + 'static {
    fn check_and_consume<'a>(
        &'a self,
        request: RateLimitRequest<'a>,
    ) -> RateLimitFuture<'a>;
}
```

The SPI is object-safe. One future atomically decides and consumes one cost for
one key under one immutable policy snapshot. It adds no split check/consume,
reserve/commit, or permit/release methods. Concurrency permits remain a separate
concern under ADR-0019.

### 2.3 Type invariants

- `OperationCost` is a `u64` of at least one and reports checked addition or
  multiplication overflow as an error.
- `QuotaKey` retains ADR-0011's component and byte bounds, distinction between
  empty and missing components, and redacted `Debug`.
- Algorithm-policy constructors reject zero, unrepresentable durations,
  overflow, and statically known over-cost. They expose no set of optional fields.
- `RateLimitRequest` borrows its key and owns a cost plus a copyable closed
  `RatePolicy` snapshot. It contains no raw principal or token.
- Decision constructors validate consistency among allowed/rejected status,
  limit, remaining, reset, retry, and policy revision.
- Key capacity, policy mismatch, and dynamic over-cost are closed rejection
  reasons that leave state unchanged, not provider failures.
- `RateLimitProviderError` retains a source chain for operators, but its `Debug`
  and `Display` disclose no source, key, or policy value.

`ProviderFailurePolicy`, `TimedRateLimiter`, Stage A and Stage B extractors and
plugins, operation-cost policy, and observer binding remain in `pythia_policy`:
they are endpoint composition rather than provider SPI. `pythia_policy` consumes
the types from `pythia_rate`.

### 2.4 Façade and direct-consumer surface

The ordinary application path is `pythia::rate`:

```rust,ignore
use pythia::rate::{RateLimitRequest, RateLimiter};
```

An independent provider crate can depend directly on the exact release version
of `pythia_rate`, or on the same 40-character Git SHA, without using the façade.

```toml
[dependencies]
pythia_rate = "=0.1.0"
```

`pythia_policy` does not compatibility-re-export SPI types. Repository
consumers, examples, compile fixtures, and rustdoc migrate to the new imports in
one change. Generated source does not refer to the rate SPI, so generated ABI 5
remains unchanged.

### 2.5 Provider conformance

The public provider suite in `pythia_testkit` evaluates each `RateLimiter`
implementation for:

- atomicity of allowed/rejected decisions and complete cost consumption;
- no limit overshoot under multi-producer concurrency;
- a `Send` future that can be dropped on timeout or cancellation;
- non-blocking method invocation and every future poll;
- watchdog coverage of repeated polls after self-wakes;
- unchanged state after malformed decisions, policy mismatch, capacity
  rejection, or dynamic over-cost; and
- `Debug` redaction of sources, keys, and policy values.

Clock semantics, network consistency, and expiry storage are provider-specific.
The suite applies additional time and expiry tests only to providers declaring
those capabilities; it does not require every third-party provider to copy the
official in-memory implementation.

## 3. Migration

| Before | After |
| --- | --- |
| `pythia_policy::RateLimiter` | `pythia_rate::RateLimiter` or `pythia::rate::RateLimiter` |
| `pythia_policy::RateLimitRequest` | `pythia_rate::RateLimitRequest` |
| `pythia_policy::QuotaKey` | `pythia_rate::QuotaKey` or `pythia::rate::QuotaKey` |
| `pythia_policy::OperationCost` | `pythia_rate::OperationCost` or `pythia::rate::OperationCost` |
| `pythia_http::BoxFuture` in an SPI implementation | `pythia_rate::RateLimitFuture` |

Because this is a pre-release change, it adds no deprecated alias, feature
switch, dual trait, or blanket adapter.

## 4. Acceptance matrix

| ID | Expected |
| --- | --- |
| RSPI-01 | `pythia_rate` has zero normal dependencies and passes on MSRV and stable. |
| RSPI-02 | The public trait is object-safe, `Send + Sync`, and can return a future borrowing its request. |
| RSPI-03 | A third-party fixture implements a provider with only `pythia_rate` as a normal dependency. |
| RSPI-04 | The SPI surface contains no split check/consume, permit/release, HTTP, or runtime types. |
| RSPI-05 | Cost, key, policy, and decision constructors reject zero, overflow, and inconsistent values. |
| RSPI-06 | Capacity, policy mismatch, and dynamic over-cost are expressible as state-preserving rejections. |
| RSPI-07 | Provider errors and all public `Debug` output disclose no source, identity, or quota value. |
| RSPI-08 | Conformance rejects non-atomic, blocking, malformed, and non-droppable providers. |
| RSPI-09 | Stage A, Stage B, failure policy, timeout, and observer binding remain in `pythia_policy`. |
| RSPI-10 | The façade re-exports the SPI from `pythia::rate`, so ordinary use requires no lower-level import. |
| RSPI-11 | `pythia_policy` contains no old SPI re-export or compatibility shim. |
| RSPI-12 | Generated source and the ABI guard remain at 5 with zero drift. |

## 5. Consequences

### Positive

- Distributed and custom providers can avoid GraphQL runtime dependencies.
- Security and concurrency review of the SPI covers a smaller surface.
- The official in-memory implementation and provider contract can be versioned
  and reviewed independently.

### Negative

- The workspace, release artifacts, and SBOM contain another crate.
- Pre-release consumers must update imports together.
- Because algorithm policies are part of the SPI, adding an algorithm changes
  the public provider contract.

## 6. Rejected alternatives

- Split only modules inside `pythia_policy`: third-party providers retain
  runtime dependencies.
- Put the trait only in the façade: provider crates inherit endpoint and
  observability dependencies.
- Define a trait per algorithm: Stage A, Stage B, and testkit paths branch.
- Use an async-runtime-specific trait: provider and application runtimes can no
  longer vary independently.
