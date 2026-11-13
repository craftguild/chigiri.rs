# ADR-0019: Make official rate limiters interchangeable through one SPI

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0011, ADR-0016, ADR-0017
- Coordinates: ADR-0022, ADR-0023, ADR-0024
- Revisits: `pythia_policy::RateLimitPolicy` and the required rate configuration
  in the production preset

## 1. Context

Pythia has two-stage rate admission and a provider SPI. With only one standard
implementation, users must repeatedly implement common requirements such as
fixed windows, burst absorption, and rolling quotas. Conversely, if the
framework infers principals, tenants, proxies, or distributed topology and
enables rate limiting automatically, it creates false confidence about the
identity and scope being protected.

Official algorithms need to be interchangeable, concise to configure in the
ordinary case, and replaceable through the same typed component graph when a
product has specialized requirements.

## 2. Decision

### 2.1 Integration surface

Official types are exported from `chigiri::rate` and configured in the single
graph from ADR-0017.

<!-- compile-check: adr-0019-official-rate:start -->
```rust
let stage_b = chigiri::rate::operation(OperationQuotaKey::new())
    .sliding_window_counter()
    .limit(10_000)?
    .window(Duration::from_secs(60))?
    .build()?;

let endpoint = chigiri::Endpoint::builder(resolvers)
    .rate_limit(
        chigiri::rate::transport(TransportQuotaKey::new())
            .token_bucket()
            .capacity(120)?
            .refill(60, Duration::from_secs(60))?
            .build()?,
    )
    .inject(|components| {
        components.rate_limit().stage_b(stage_b);
    })
    .without_context()
    .build()?;
```
<!-- compile-check: adr-0019-official-rate:end -->

`.rate_limit(stage)` carries the stage kind in its type and delegates internally
to either `components.rate_limit().stage_a(...)` or `stage_b(...)`. The last
configuration of a stage wins, while Stage A and Stage B coexist. The ordering
from ADR-0011 remains unchanged: Stage A runs before the body is polled; Stage B
runs after validated and coerced operation cost is known and before any
resolver.

The automatic baseline from ADR-0016 leaves rate limiting unconfigured because
it cannot infer application identity. An official stage builder defaults to
fail closed on provider failure and a 500 ms provider timeout. Typed injection
can change these values explicitly; an omitted failure policy never implies
fail open.

### 2.2 Common SPI

Every algorithm and third-party provider implements the same `RateLimiter` SPI,
which atomically decides and consumes a cost for one quota key. A decision
contains at least allowed or rejected status, a bounded `retry_after`, and a
policy revision. It contains no secret key or raw identity.

- One request is not split into separate read and update calls.
- Provider futures are `Send`, can be dropped on timeout or cancellation, and
  require no detached task.
- The façade binds its final error observer to Stage A and Stage B regardless of
  configuration order and reports each fail-open or fail-closed provider failure
  exactly once.
- `check_and_consume` and every wake-triggered poll of its returned future are
  non-blocking and return promptly. The 500 ms timeout is cooperative competition
  between futures on the same task, not a hard deadline that can preempt
  synchronous blocking. A provider that performs blocking I/O violates
  conformance; if necessary, it returns a non-blocking future backed by bounded
  workers or isolation that it owns.
- The clock is an injectable monotonic contract. Wall-clock rollback never
  increases quota.
- Keys, costs, durations, and capacities use bounded newtypes.
- Exhausted key capacity and a policy mismatch for an existing key are explicit
  rejections, not provider failures subject to fail-open.
- A quota rejection for an existing key with the same policy does not consume or
  reset tokens, counters, or levels. It updates only `last_seen` for bounded-store
  liveness. Timeout, cancellation, malformed decisions, policy mismatch, and key
  capacity rejection change neither quota accounting nor liveness.

The existing policy type is not expanded into an ambiguous set of optional
algorithm-specific fields. It is redesigned as common metadata plus a sealed
algorithm-policy enum. Because the project has not reached a production release,
the old `RateLimitPolicy` receives no compatibility shim.

### 2.3 Official in-memory algorithms

The standard in-memory provider offers four algorithms. Key count, idle
lifetime, and every numeric setting are bounded.

#### Token bucket

State contains the current token count and last refill tick. Only complete
elapsed refill intervals add `tokens_per_interval`, clamped to capacity. A
decision subtracts the cost exactly once when the post-refill token count is
sufficient. Otherwise, `retry_after` is the checked ceiling to the first
interval at which the required tokens are available.

#### Fixed window

The window index is `elapsed / window` from a monotonic epoch. The first
decision after the index changes resets the counter to zero. The current cost is
consumed only when the sum is at most the limit. A boundary tick belongs to the
new window. A rejection reports the duration to the next boundary.

#### Sliding-window counter

State contains counters for the previous and current windows. For window width
`W` and elapsed time `e` in the current window, checked `u128` arithmetic
compares the following without floating point:

```text
estimated_scaled = current * W + previous * (W - e)
candidate_scaled = estimated_scaled + cost * W
allowed          = candidate_scaled <= limit * W
```

Advancing by at least one window rotates state as appropriate; advancing by two
or more windows sets `previous` to zero. Overflow is a construction error or an
explicit rejection, not a provider error, and leaves state unchanged.
`retry_after` is the first positive tick at which the same cost would pass if no
additional consumption occurs. It is solved with checked integer ceilings over
the remainder of the current window and the linear decay in the next window. It
must not simply report a boundary at which the request would still be rejected.

#### Leaky bucket meter

This is a meter without queues or sleeps. Its policy contains
`units_per_interval` and `leak_interval`. The number of complete elapsed
intervals is multiplied by the units with checked arithmetic, reducing the level
toward zero. A cost is added when `level + cost <= capacity`. Otherwise,
`retry_after` is the checked ceiling to the first interval that drains the
required units. Requests are never delayed, reordered, or moved to background
tasks.

For every algorithm, `cost > capacity/limit` is rejected at construction when
known statically. A dynamic request-derived over-cost is an explicit rejection
that leaves state unchanged.

### 2.4 Memory and expiry

Provider construction requires bounded `max_keys` and `idle_ttl` values or safe
defaults. Expired keys may be removed at the next safe maintenance point, but
the request hot path does not scan the whole map. Reaching capacity explicitly
rejects a new key instead of randomly evicting an unrelated active key and
resetting its quota.

Locks cover only the atomic decision for one key and are never held across an
await. Shard count and hash seed are internal details, but adversarial keys
cannot cause unbounded allocation.

### 2.5 Concurrency limiting is separate

Concurrency limits differ from time quotas because their permits must be
returned when a request ends or is dropped. They remain in the existing Query
concurrency boundary of `ExecutionLimits`; `RateLimiter` does not mix in permit,
release, or in-flight APIs. This ADR adds no public concurrency-provider SPI or
algorithm. A future addition requires a separate ADR and typed component. Stage
A or Stage B tokens are not rolled back when a request later fails.

### 2.6 Third-party providers

Distributed providers such as Redis are not part of the first official set.
Consumer crates can implement them through the public SPI and the
`pythia_testkit` conformance suite. The suite checks atomicity, concurrency,
timeout drop, clock rollback, expiry, capacity, policy mismatch, malformed
decisions, and `Debug` redaction. Its blocking watchdog drives repeated polls
after self-wakes, not only the first poll, and rejects providers that block
synchronously on a later poll.

ADR-0022 and ADR-0023 preserve these semantics while separating the SPI into
the std-only `pythia_rate` crate and the four official implementations into
`pythia_rate_memory`. Stage A, Stage B, and the ordinary `pythia::rate` sugar
remain as defined here. The pre-release migration leaves no old
`pythia_policy` re-export.

## 3. Acceptance matrix

| ID | Expected |
| --- | --- |
| LIMIT-01 | Token bucket handles capacity, refill boundaries, checked retry, and dynamic over-cost exactly. |
| LIMIT-02 | Fixed window assigns a boundary tick to the new window and never exceeds the limit under concurrent consumption. |
| LIMIT-03 | Sliding counter follows the integer formula, rotation, overflow, and retry contract in Section 2.3. |
| LIMIT-04 | Leaky bucket drains by interval and never delays, queues, or backgrounds a request. |
| LIMIT-05 | All four algorithms bound maximum keys, idle expiry, and capacity rejection; an active key rejected for quota updates only liveness, not accounting. |
| LIMIT-06 | A policy mismatch for the same key and exhausted key capacity are explicit rejections that leave state unchanged. |
| LIMIT-07 | Provider error and timeout fail closed by default; only explicit fail-open configuration continues the request. |
| LIMIT-08 | Timeout and cancellation drop the provider future directly and leave no detached task. |
| LIMIT-09 | A Stage A rejection polls no body, a Stage B rejection starts no resolver, and Stage A consumption is not rolled back. |
| LIMIT-10 | Sugar and typed injection produce the same stage snapshot, and the last configuration of one stage wins. |
| LIMIT-11 | The automatic baseline reports rate coverage as unconfigured while retaining structural limits. |
| LIMIT-12 | Quota keys, `Debug`, observers, and errors disclose no credential, raw identity, or provider source; policy, snapshot, and config `Debug` disclose no quota values. |
| LIMIT-13 | Rollback, equal ticks, and large jumps in the injected monotonic clock do not over-recover quota. |
| LIMIT-14 | Third-party provider conformance rejects non-atomic, malformed, and blocking implementations. |
| LIMIT-15 | The existing Query concurrency boundary remains, and permit/release semantics do not enter the time-quota SPI or policy. |
| LIMIT-16 | Public API contains neither an old-policy shim nor invalid combinations of algorithm-specific optional fields. |

## 4. Consequences

### Positive

- Applications can select a common quota algorithm and replace it within the
  same stage.
- Ordinary configuration remains concise while deep control of providers,
  failures, and clocks stays on the same builder.
- Algorithm boundaries and the testkit make third-party distributed providers
  assessable.

### Negative

- Boundary, concurrency, and security tests for four algorithms require ongoing
  maintenance.
- An in-memory provider cannot enforce a quota across multiple processes.
- Last-write-wins configuration requires effective-configuration auditing.

## 5. Rejected alternatives

- Provide only token bucket: consumers must reimplement fixed contracts and
  rolling quotas.
- Make leaky bucket a waiting queue: endpoint deadlines, ordering, and
  cancellation ownership become more complex.
- Add Redis as a core dependency: runtime and network choices move into the
  framework.
- Enable one anonymous global rate limit for every application: the protection
  scope is easily misunderstood.
