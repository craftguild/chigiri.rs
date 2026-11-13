# ADR-0011: Separate structural budgets from two-stage rate limiting

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-28
- Decision owners: Pythia maintainers
- Depends on: ADR-0009, ADR-0010
- Coordinates: ADR-0019, ADR-0022, ADR-0023
- Revisits: ADR-0006

## 1. Context

Pythia already limits document bytes, depth, expanded fields, aliases,
directives, variables, selection-set concurrency, output, and time. Production
quotas also need coarse protection before reading a request body and
operation-level control based on principal, tenant, and operation cost.

One `rate_limit(request)` cannot satisfy both needs: before reading the body the
operation is unknown, while waiting until operation selection allows an
attacker's body to be read first. Embedding IP interpretation, trusted-proxy
policy, tenant plans, or Redis topology in the framework would also optimize it
for one application.

## 2. Decision

### 2.1 Four independent controls

Resource governance is divided into four independent categories.

| Control | Example | Owner |
| --- | --- | --- |
| structural limit | body/document/response bytes, depth, fields, aliases, list/output node | Pythia endpoint/core |
| concurrency/deadline | connections, request phase, in-flight fields | application server / endpoint / core |
| operation cost | schema coordinate weight, catalog cost metadata | `pythia_policy` |
| quota/rate | tokens/windows for each subject | provider/application |

Rate limits do not replace structural limits: existing production limits remain
active even when no rate limiter is configured. Conversely, depth is not treated
as remaining quota.

### 2.2 Optional policy crate

The public rate/cost contract and its standard implementation belong in the
independent `pythia_policy` crate. Consumers that do not use this crate do not
gain clock, random-number, Redis, or HTTP-framework dependencies.

`pythia_core` exposes only the structural summary and schema-coordinate metadata
already computed during validation. Provider I/O, principals, quota plans, and
HTTP headers do not enter the core.

ADR-0019 later expands the official algorithms to four. ADR-0022 and ADR-0023
separate the provider SPI into the std-only `pythia_rate` crate and official
storage implementations into `pythia_rate_memory`. This section's boundary—no
provider in the core—remains in force, while the later ADRs take precedence for
crate placement and the number of official implementations.

### 2.3 Cost model

Operation cost is a bounded unsigned 64-bit integer. All addition and
multiplication uses checked arithmetic; overflow causes policy rejection rather
than saturation. Every executable operation has a minimum cost of 1.

`OperationCostPolicy` deterministically computes cost from a validated operation
and these inputs:

- operation kind and operation key
- expanded field/alias/depth summary
- schema coordinate
- argument type, presence, and bounded scalar metadata
- a validated cost hint registered by the application in the ADR-0010 catalog

The default policy does not receive secret variable values or arbitrary
application objects. A custom policy that uses argument values, for example for
list pagination, identifies the target coordinates and permitted scalar
arguments and defines a maximum multiplier.

A list multiplier applies to the field's entire child selection, including
fragments and inline fragments, not just to that field. Nested lists multiply
ancestor factors with checked arithmetic and reject overflow. Literals,
variables, variable defaults, and schema argument defaults use the same coerced
metadata. A requested value above the configured maximum is rejected with
`OPERATION_COST_EXCEEDED` before resolver execution; the charged value is not
silently clamped.

A catalog cost hint is a final worst-case cost that the application has
prevalidated across every argument range permitted by the catalog document and
configured list rules. In the standard policy it replaces the normal
coordinate-weight calculation, but it does not replace validation of each
request-dependent multiplier's type and value, coordinate maxima, checked
ancestor multiplication, or nested overflow. The hint is used only after these
checks and never exempts dynamic input from structural guards.

SDL custom directives are not automatically interpreted as costs. Standardizing
a directive such as `@cost` requires a separate ADR covering its definition,
introspection, catalog metadata, and validation. Until then, applications use a
code/configuration coordinate map.

The standard policy provides:

- unit field cost
- fixed weight for each coordinate
- bounded list multiplier
- catalog metadata override for each operation key

### 2.4 Two-stage rate limiting

#### Stage A: transport rate limit

Stage A runs in ADR-0009 phase 2, before the body is read. An application
extractor derives a bounded subject key from neutral transport metadata and
consumes one unit from a coarse quota.

This stage provides burst protection by peer, API key, or session. Pythia never
interprets IP headers automatically. The application explicitly chooses whether
subject-extraction failure uses an anonymous bucket, rejects the request, or is
treated as an internal failure.

#### Stage B: operation quota

In ADR-0009 phase 11, an application extractor derives a principal, tenant, or
plan key from context `C`, then consumes quota using the operation key and
`OperationCost`. Resolver execution starts only after quota admission succeeds.

Stages A and B may use separate buckets, providers, and failure policies.
Consumption by both stages for one request is normal, and a Stage B rejection
does not roll back Stage A consumption.

### 2.5 Bounded keys

The framework's `QuotaKey` constructor enforces these rules:

- Every namespace, subject, and operation-class component is explicitly UTF-8
  or opaque bytes.
- Component count, each component's byte length, and total byte length are
  bounded.
- Secret raw tokens are neither stored directly nor exposed by `Debug`.
- An empty subject differs from a missing subject.

Applications pass HMAC- or hash-derived subjects when needed. Pythia does not
automatically hash authorization headers, cookies, or email addresses into quota
keys.

### 2.6 Provider SPI

`RateLimiter` is a conceptual atomic decision.

```text
check_and_consume(key, cost, policy, deadline)
  -> Allowed { limit, remaining, reset }
   | Rejected { limit, remaining, retry_after, reset }
   | ProviderFailure
```

Checking and consuming are not separate methods. The provider contract requires
that concurrent requests cannot exceed the limit and that a request's full cost
is consumed atomically. Provider futures are `Send` and can be dropped at the
deadline.

The provider owns its clock but permits injection of a test clock. Provider
conformance tests cover backward wall-clock movement. Provider documentation
declares distributed strictness, eventual consistency, clock source, and key
expiry.

Standard implementations are limited to:

- bounded in-memory token bucket
- deterministic test clock/provider

Redis and other stores belong in separate provider crates or applications; the
in-memory provider is not recommended as a multi-instance global quota. In the
standard in-memory provider, key-capacity exhaustion and a different policy
snapshot for a retained key return `Rejected` without modifying the existing
bucket. A different policy is accepted only after the original bucket expires.
This prevents fail-open admission of new subjects and implicit policy resets
that would replenish an existing key's quota.

### 2.7 Failure policy

Fail-open or fail-closed behavior is required when each limiter is registered;
there is no implicit default.

- Availability protection/security quota recommends fail-closed.
- telemetry-only shadow limiter can choose fail-open.
- A provider timeout must not be aggregated into the same metric as an allowed
  decision.
- Even in fail-open mode, internal provider errors are reported to the observer.

Invalid quota configuration, a zero window, overflow, or an unsupported
algorithm fails endpoint construction or application startup rather than
falling back at request time.

### 2.8 Rejection contract

Rate-limit rejection is an HTTP 429 GraphQL request error with
`extensions.code = RATE_LIMITED`. Client-visible metadata is limited to:

- either a retry delay or an absolute reset time;
- limit/remaining, authorized by the policy.
- a stable public scope name.

Quota keys, principals, internal tenant-plan names, and provider errors are not
returned. The endpoint adds HTTP `Retry-After` only when it can represent the
value correctly and consistently with the GraphQL extensions. No other
rate-limit header is reserved without a standardizing ADR.

### 2.9 Concurrency limit

Rate is not the same as simultaneous execution count. Application-server
connection and in-flight request semaphores and core selection-set field limits
remain in force. A per-tenant concurrency limiter uses a separate provider
contract with permit lifetime and release-on-drop semantics; it is not disguised
as a `RateLimiter`.

### 2.10 Accepted Rust contract

The `pythia_policy` public contract is fixed as follows:

- `OperationCost`: a `u64` newtype greater than zero. `checked_add` and
  `checked_mul` report overflow as `OperationCostError`.
- `OperationCostPolicy`: an object-safe trait that synchronously and
  deterministically returns cost from a read-only validated operation/cost view.
- `QuotaKey`: an owned bounded key containing namespace, subject, and optional
  operation class. Three components, 128 bytes per component, and 384 bytes
  total are hard limits; `Debug` never displays component bytes.
- `RateLimiter`: an object-safe async trait with only `check_and_consume`.
- `RateLimitDecision`: `Allowed` or `Rejected`; consistency constructors validate
  limit, remaining, reset, and optional retry delay.
- `Clock`, `FakeClock`, and `InMemoryTokenBucket`: use monotonic ticks and bound
  capacity, key count, and idle expiry through a builder. Key-capacity exhaustion
  and policy-snapshot mismatch reject without changing an existing bucket.
- `ProviderFailurePolicy`: `FailOpen` / `FailClosed` must be specified when registering.

Stage A and B timeouts are not delegated to provider-spawned tasks; endpoint and
operation-admission futures directly poll and drop provider futures. A
fail-closed provider failure is 503 with `RATE_LIMIT_UNAVAILABLE`; ordinary
rejection is 429 with `RATE_LIMITED`. Cost-policy limit excess or overflow is a
pre-resolver 422 (or 200 for legacy JSON under the existing request-error rule)
with `OPERATION_COST_EXCEEDED`.

## 3. Configuration boundary

Pythia does not automatically read policy values from environment variables.
The application passes these values to the typed builder:

- limiter/provider
- extractor
- algorithm/window/burst
- fail policy
- timeout
- public scope/retry metadata policy

Reading values from a configuration file, secret manager, or feature flag is an
application responsibility. A provider that permits runtime configuration
updates attaches a snapshot version to decisions and events and never mixes
multiple versions within one request.

## 4. Alternatives considered

### Recommend only Axum/Tower rate-limit middleware

It is suitable for pre-body limits, but cannot safely use validated operations,
cost, and tenant context or share a policy contract across bridges. It is not
the standard solution, although it may serve as a bridge to a Stage A provider.

### Make Redis a required standard Pythia dependency

This would fix deployment topology and failure semantics while imposing an
unnecessary dependency on consumers that use a single process or another
gateway.

### Control every resource through weighted complexity

Body/parser cost, I/O cost, bursts, and concurrency cannot be represented
accurately by one number.

## 5. Consequences

### Positive

- Body attacks and high-cost operations can be rejected at their respective earliest useful phases.
- Rate limiting is standardized without adding application-specific IP, session, or tenant definitions to the framework.
- Replacing a provider preserves the 429, error, and cancellation contracts.
- Operation catalogs and observability can share the same identity and cost.

### Negative

- Applications must explicitly implement subject extractors and quota policies.
- Operators must understand that one request can consume two buckets.
- Distributed correctness depends on provider conformance and deployment.

## 6. Acceptance matrix

| ID | Expected |
| --- | --- |
| BUDGET-01 | Structural limits and operation cost reject independently; list-limit excess is never clamped and is rejected before resolver execution regardless of catalog hints. |
| BUDGET-02 | List multipliers accumulate over the complete child selection regardless of catalog hints, and nested checked-arithmetic overflow is a pre-resolver policy rejection. |
| RATE-01 | Stage A rejection causes zero body polls, context construction, and execution. |
| RATE-02 | Stage B rejection causes zero resolver calls and does not roll back Stage A consumption. |
| RATE-03 | Concurrent consumption never exceeds the provider limit. |
| RATE-04 | Provider timeout drops its future and follows the configured failure policy. |
| RATE-05 | The 429 body/code and `Retry-After` preserve media negotiation. |
| RATE-06 | Quota keys, tokens, and internal tenant values never enter responses or log labels. |
| RATE-07 | The in-memory provider bounds capacity, key count, and expiry and explicitly rejects key exhaustion or policy mismatch. |
| RATE-08 | Existing production structural limits remain active when no limiter is configured. |

Before moving to Accepted, public-contract-first fixtures must fix the provider
trait, key and cost types, exact 429 response, in-memory algorithm, and fake
clock contract.
