# ADR-0023: Provide official rate limiters in an in-memory adapter crate

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-30
- Decision owners: Pythia maintainers
- Depends on: ADR-0011, ADR-0019, ADR-0022
- Compatibility: pre-release breaking change

## 1. Context

Pythia already maintains official token-bucket, fixed-window,
sliding-window-counter, and leaky-bucket-meter implementations. Once ADR-0022
separates `RateLimiter` into a minimal SPI crate, the official storage
implementation should also remain outside the SPI and be identified explicitly
as a process-local adapter. Splitting every official algorithm into its own
crate, however, would add package management and choices without useful
separation.

In-memory storage is the only backend maintained officially by Pythia. Redis,
databases, gateways, sidecars, and other backends belong in consumer or provider
crates implementing the same SPI.

## 2. Decision

### 2.1 One official adapter crate

Add `pythia_rate_memory` to the workspace. Its only normal dependencies are
`pythia_rate` and `atomic-waker`, used solely for lock-free waiter notification.
It does not depend on an async runtime, HTTP, the GraphQL runtime, Serde, a
network client, or a background executor. A standard `Mutex` alone cannot
register multiple futures' `Waker`s without synchronous waiting or `unsafe`, so
notification cells use the smallest dependency with a safe API. No quota,
policy, SPI, or executor functionality is imported from that dependency.

The crate exports four providers:

- `InMemoryTokenBucket<C = SystemClock>`
- `InMemoryFixedWindow<C = SystemClock>`
- `InMemorySlidingWindowCounter<C = SystemClock>`
- `InMemoryLeakyBucket<C = SystemClock>`

Each preserves the formulas, boundaries, retry rules, and non-mutating rejection
semantics in ADR-0019 Section 2.3. Algorithms are not reimplemented as rate
stages, endpoints, or HTTP middleware.

### 2.2 Safe default configuration

Common configuration has this fixed shape:

```rust,ignore
pub struct InMemoryRateLimitConfig {
    // Stores max_keys and idle_ttl as validated private fields.
}
```

`Default` uses these safe process-local values:

| Setting | Default |
| --- | ---: |
| `max_keys` | 10,000 |
| `idle_ttl` | 300 seconds |

`InMemoryRateLimitConfig::new(max_keys, idle_ttl)` rejects zero,
unrepresentable durations, and overflow during conversion to internal ticks.
Provider construction is consistent:

```rust,ignore
let standard = InMemoryTokenBucket::new();
let configured = InMemoryTokenBucket::with_config(config);
let deterministic = InMemoryTokenBucket::with_clock(config, clock);
```

- `new()` and `Default` use the same safe configuration and `SystemClock`.
- `with_config` accepts validated configuration and uses `SystemClock`.
- `with_clock` accepts validated configuration and a test or custom monotonic
  clock.
- No setter changes maximum keys, TTL, or clock after construction.

Official façade sugar uses the safe default internally; it does not require
users to select `production_defaults()` or provide memory configuration. Typed
deep injection can replace it with an explicit provider and configuration.

### 2.3 Clock contract

`pythia_rate_memory` exports `MonotonicTick`, `Clock`, `SystemClock`, and the
thread-safe `ManualClock`.

- A tick is a bounded monotonically non-decreasing integer. No wall-clock
  timestamp or system time is exposed through responses or decisions.
- A provider treats a tick lower than the previous tick as equal to the previous
  tick and recovers no quota.
- An equal tick performs no refill, window rotation, or drain.
- Large-jump multiplication, window indexes, and retry calculations use checked
  arithmetic.
- `ManualClock` creates deterministic advance and rollback scenarios for tests
  and conformance. The production façade does not use it.

Provider state locks are not held while calling a clock. A custom clock that
panics or blocks is an application failure or conformance violation; providers
do not hide it through catching or spawning.

### 2.4 Bounded store and atomicity

- Key count never exceeds `max_keys`. At capacity, the provider returns a closed
  capacity rejection rather than evicting an active key randomly or by LRU and
  resetting its quota.
- Idle expiry uses a bounded maintenance index. The request hot path never scans
  the whole map.
- A quota rejection for an existing key leaves accounting unchanged and updates
  only liveness to the current tick.
- Capacity, policy mismatch, dynamic over-cost, timeout, cancellation, and a
  malformed result change neither accounting nor liveness.
- Refill, rotation, or drain, followed by decision and consumption for one key,
  occurs in one critical section. No lock is held across await, sleep, or I/O.
- On lock contention, a future poll returns `Pending` instead of waiting
  synchronously. A waiter is registered in one of at most 64 fixed slots, and
  lock release wakes exactly one. The lock is rechecked after registration to
  avoid a lost wake. If a notified future is dropped, it reacquires and releases
  the lock so notification passes to the next waiter. Dropping a future clears
  its slot and `Waker` without changing accounting state.
- The hard limits are 64 waiters and one selected waiter slot per lock release.
  Including `AtomicWaker` register/wake races, user `Waker` callbacks are limited
  to two per release, and the complete cancellation relay is bounded by the 64
  waiter slots. A request beyond the limit neither waits synchronously nor
  self-wakes; it returns a redacted provider failure through the default
  fail-closed path.
- Adversarial keys cannot cause unbounded allocation, cleanup, or rehash retries
  per request.
- Poisoning and other internal store failures become redacted
  `RateLimitProviderError`s.

Leaky bucket is a meter, not a request queue for quota admission. It has no
permit, sleep, worker, or background task. The bounded waiters above notify
mutex release for all four providers; they do not represent quota or time
waiting. None of the providers releases state at request completion or acts as
a concurrency limiter.

### 2.5 Public exposure and composition

Direct use has one path:

```rust,ignore
use pythia::rate::memory::{InMemoryRateLimitConfig, InMemoryTokenBucket};
```

Independent provider and adapter consumers can depend on
`pythia_rate_memory` directly. `pythia_policy` does not re-export official
providers, clocks, or configuration. Existing façade sugar remains:

```rust,ignore
let stage = pythia::rate::transport(extractor)
    .token_bucket()
    .capacity(120)?
    .refill(60, Duration::from_secs(60))?
    .build()?;
```

Sugar creates a `pythia_rate_memory` provider and passes it to the same typed
component graph from ADR-0017. Stage A and Stage B, last-write-wins within one
stage, default fail-closed behavior, the 500 ms provider timeout, and observer
binding remain unchanged.

### 2.6 Packaging and performance evidence

Add `pythia_rate` and `pythia_rate_memory` to exact workspace versions, release
packages, source archives, fresh-consumer checks, SBOM components and dependency
edges, provenance, and checksums. The package verifier derives a closed expected
set from the workspace and checks bidirectional equality with the artifacts; it
does not silently accept an old package count.

Because crate separation changes hot-path linking and layout, existing formal
benchmark, load/soak, and long-fuzz results are not reused as release evidence
for the new HEAD. The implementation agent runs only short regressions and
records reacquisition commands, owners, and incomplete Gates P-D and P-E in the
manual runbook.

## 3. Migration

| Before | After |
| --- | --- |
| `pythia_policy::InMemoryTokenBucket` | `pythia_rate_memory::InMemoryTokenBucket` or `pythia::rate::memory::InMemoryTokenBucket` |
| `pythia_policy::InMemoryFixedWindow` | `pythia_rate_memory::InMemoryFixedWindow` |
| `pythia_policy::InMemorySlidingWindowCounter` | `pythia_rate_memory::InMemorySlidingWindowCounter` |
| `pythia_policy::InMemoryLeakyBucket` | `pythia_rate_memory::InMemoryLeakyBucket` |
| `pythia_policy::{Clock, FakeClock, SystemClock}` | `pythia_rate_memory::{Clock, ManualClock, SystemClock}` |
| Constructor-specific `max_keys, idle_ttl` | Validated `InMemoryRateLimitConfig` |

No compatibility alias, old-constructor shim, or feature-selected duplicate
implementation is added.

## 4. Acceptance matrix

| ID | Expected |
| --- | --- |
| RMEM-01 | The crate's only normal dependencies are `atomic-waker` and `pythia_rate`, and it passes on MSRV and stable. |
| RMEM-02 | `new()` and `Default` use the same safe configuration of 10,000 keys and a 300-second TTL. |
| RMEM-03 | Configuration rejects zero and duration or tick overflow during construction. |
| RMEM-04 | Token bucket handles capacity, refill, retry, and dynamic over-cost exactly. |
| RMEM-05 | Fixed window assigns a boundary tick to the new window and never exceeds the limit under concurrent consumption. |
| RMEM-06 | Sliding counter follows ADR-0019's integer formula, rotation, overflow, and retry semantics. |
| RMEM-07 | Leaky meter drains by interval and creates no queue, wait, task, or permit. |
| RMEM-08 | All four providers bound maximum keys and idle expiry without a full-map scan or random eviction. |
| RMEM-09 | Only quota rejection updates liveness while leaving accounting unchanged. |
| RMEM-10 | Capacity, mismatch, over-cost, timeout, and cancellation change neither accounting nor liveness. |
| RMEM-11 | Rollback, equal ticks, and large jumps do not over-recover quota, and overflow is rejected without changing state. |
| RMEM-12 | Multi-producer conformance never exceeds the atomic limit, never self-wakes under contention, wakes only a bounded number on lock release, and progresses after drop. |
| RMEM-13 | Façade sugar and deep injection of the same provider produce identical results. |
| RMEM-14 | `pythia_policy` has no old provider, clock, or configuration export or compatibility shim. |
| RMEM-15 | Package, source, SBOM, provenance, and fresh-consumer checks exactly verify the two new crates. |
| RMEM-16 | Formal performance evidence is invalidated and its manual reacquisition TODOs and incomplete gates remain. |

## 5. Consequences

### Positive

- Ordinary users can select a safe process-local implementation without extra
  configuration.
- Dependency and review boundaries separate third-party providers from official
  storage.
- All four algorithms share one configuration, clock, and bounded-store
  contract.

### Negative

- A different provider is required for multi-process or global quotas.
- Another crate and release artifact are added, and consumer imports change
  incompatibly.
- Safe lock-release notification adds `atomic-waker` to the direct maintenance
  surface.
- Safe defaults configure algorithm storage, not application-specific quota
  values.

## 6. Rejected alternatives

- Include Redis or another remote backend in the official crate: the framework
  would choose deployment, runtime, and failure topology.
- Create one crate per algorithm: package selection and release management grow.
- Use an unbounded map or random eviction: quota bypass and memory denial of
  service become possible.
- Run a background cleanup task: the adapter acquires an async-runtime and
  shutdown-ownership requirement.
- Combine rate and concurrency in one provider: permit lifetime conflicts with
  token-consumption semantics.
