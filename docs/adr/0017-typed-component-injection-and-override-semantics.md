# ADR-0017: Apply sugar methods and typed component injection to one configuration graph

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0016
- Coordinates: ADR-0018, ADR-0019

## 1. Context

Most users need a small API that can be configured through IDE completion alone,
while others need fine-grained control over authentication, rate limiting,
operation sources, diagnostics, and plugins. Splitting these needs between a
simple entry point and an `advanced` entry point makes it unclear which surface
to choose and encourages the two construction paths to acquire different
security and execution semantics.

An effective configuration is also difficult to predict locally when repeated
setter calls produce duplicate errors for some components, use first-write-wins
for others, and use last-write-wins elsewhere.

## 2. Decision

### 2.1 One builder, one component graph

The `Endpoint::builder` introduced by ADR-0016 is the only construction entry
point. Common settings are exposed as sugar methods directly on the builder;
advanced settings receive a typed editor from `inject` on that same builder.

<!-- compile-check: adr-0017-one-graph:start -->
```rust
let transport_rate = chigiri::rate::transport(TransportQuotaKey)
    .token_bucket()
    .capacity(120)?
    .refill(60, Duration::from_secs(60))?
    .build()?;
let operation_rate = chigiri::rate::operation(OperationQuotaKey)
    .sliding_window_counter()
    .limit(10_000)?
    .window(Duration::from_secs(60))?
    .build()?;
let endpoint = chigiri::Endpoint::builder(resolvers)
    .body_limit(2 * 1024 * 1024)
    .rate_limit(transport_rate)
    .inject(|components| {
        components.operation().source(source);
        components.rate_limit().stage_b(operation_rate);
        components.observability().error_observer(observer);
    })
    .without_context()
    .build()?;
```
<!-- compile-check: adr-0017-one-graph:end -->

The conceptual signature of `inject` is
`fn inject(self, FnOnce(&mut Components<'_, C>)) -> Self`. It consumes and
returns the same builder through a synchronous closure. The closure is not
stored or rerun after `build()` or during a request. Fallible provider and limit
construction completes before injection. Multiple `inject` calls may be
interleaved with sugar methods; every call edits one draft component graph in
call order.

Stage A and Stage B retain an observer-binding slot. Whether
`error_observer` is configured before or after a rate stage, the final observer
is connected to provider failures. Failure events are retained even when a
fail-open policy allows the request to continue.

When the input is a prebuilt `ExecutableSchema`, the draft does not implicitly
import plugins, samplers, observers, ID factories, field instrumentation,
limits, or other execution settings previously attached to it. Only schema
identity, resolvers, scalars, and the catalog form the kernel. The endpoint
draft remains the source of truth for the execution component graph. Generated
and prebuilt inputs therefore share the same automatic baseline,
last-write-wins rules, and Stage B ordering.

`Components<'_, C>` is a closed typed editor with at least these namespaces:

- `execution()`: document, cache, completion, and concurrency resource limits.
- `http()`: body, response, deadline, CSRF, and header authority.
- `operation()`: raw and persisted sources, catalog, and audit.
- `rate_limit()`: Stage A, Stage B, provider timeout, and failure policy.
- `observability()`: IDs, sampling, lifecycle and error observers, and field
  instrumentation.
- `plugins()`: Request, Operation, and Field plugin chains.

No public API exposes an `Any` map, string service locator, global registry, or
runtime downcast for arbitrary registered types. New extension points require a
namespace and a typed method.

After a context factory is configured, `Components<'_, C, S>` treats
`http().response_hook` as a single slot typed to response state `S`. It delegates
to the same leaf operation as the builder-level `response_hook`, so the last
call wins. The method is absent from the context-pending editor, preventing a
different response-state type from becoming a runtime downcast failure. The
pending editor uses the public `ContextPending` marker. That marker deliberately
does not implement `Send + Sync`, which keeps response-state-dependent methods
off the executable surface shown by IDEs and rustdoc. Only `context_factory` or
`without_context` can transition to a concrete state `S`.

### 2.2 Sugar is an exact delegation

Each builder-level sugar method calls exactly one leaf operation on the typed
editor. It has no separate state, validation, or execution order.

```rust,ignore
// Public shape. Both methods delegate to the same internal operation.
builder.body_limit(limit)
builder.inject(|c| c.http().body_limit(limit))
```

Every sugar method requires conformance tests proving that both forms produce
the same `EffectiveConfiguration`, schema digest, request lifecycle, and
response.

### 2.3 Override rules

Configuration changes receive a monotonically increasing internal sequence.
Each single-value slot starts with its baseline at sequence zero, and the last
valid setting for that slot wins. Whether a value came from sugar or `inject`
does not affect precedence.

```rust,ignore
Endpoint::builder(resolvers)
    .body_limit(first)
    .inject(|c| c.http().body_limit(second))
    .body_limit(third)
```

The effective value above is `third`. A duplicate call is not a build error and
does not leave the first value fixed implicitly. Setter names do not use a
`replace_` prefix; setting already means replacement.

Stage A and Stage B rate limits occupy different slots. Reconfiguring the same
stage replaces it; different stages coexist. Repeated namespace access always
edits the same draft graph.

Multi-value collections do not use implicit last-write-wins. Their method names
make the operation explicit:

- `push_*` appends, and plugin or observer execution follows call order.
- `set_*` replaces the entire collection.
- `clear_*` empties only that collection. Section 2.5 applies when this weakens
  security.

Plugins with the same name are not deduplicated automatically. A dangerous
duplicate combination is either rejected by final validation or represented as
a typed single slot. `set_*` and `clear_*` replace or remove the complete final
execution collection; they do not retain a hidden collection imported from the
input schema.

### 2.4 Validate the final graph

2026-10-24 implemented amendment:
[ADR-0055](0055-subscription-configuration-and-lifecycle-ergonomics.md) permits
typed numeric constraint evidence only for subscription delivery/protocol
settings. Other slots keep the no-values rule
below; arbitrary strings and secrets remain excluded everywhere. This amendment
also retains one immutable component graph while adding SDK-managed runtime
lifecycle and optional shutdown options outside schema/execution settings.
No second endpoint builder or injection registry is introduced. The amendment
is Accepted / Implemented with local macOS evidence in the
ADR-0055 validation record.
Historical evidence retains its original source scope; this amendment does
not qualify non-local platforms or production capacity.

Individual setters validate only the range of their own value. Cross-component
validation runs in `build()` after all overrides, so a temporarily inconsistent
draft can be repaired by a later setting. A final inconsistency fails closed and
identifies the conflicting slots and origins without exposing values or
secrets.

The public `ConfigurationDiagnostic` contains closed
`ConfigurationSlot`, `ConfigurationOrigin`, and `ConfigurationViolation`
values. It contains no arbitrary strings, configuration values, reasons passed
to dangerous APIs, or provider/source errors. Body, response, and the eight
deadline types have distinct slots. `Debug` and `Display` expose only these
redacted enums.

After a successful `build()`, the component graph is immutable. Endpoint clones
have no configuration-mutation API. Dynamic policy updates use an explicit
snapshot/revision contract supplied by the relevant provider.

### 2.5 Safe weakening is conspicuous

Ordinary setters may adjust bounded limits and timeouts to product requirements.
They do not express removal of the boundary, removal of the CSRF guard, public
introspection, or disclosure of internal response details through `None`, zero,
or an empty collection. Such changes require a narrowly named method such as
`dangerously_disable_csrf_guard` and a reason string, recorded in the effective
configuration as a `SafetyOverride`. Diagnostics and `Debug` never emit the
reason automatically; an explicit read-only accessor is required. `Debug`
reveals only the protected feature name and whether a reason exists.

Invalid zero values and empty strings do not mean “unlimited.” Final validation
rejects values outside the byte and deadline hard upper bounds from ADR-0016.

### 2.6 Effective configuration

`Endpoint::configuration()` returns a read-only snapshot without secrets,
principals, raw documents, or provider-internal state. It exposes at least the
baseline value, effective value, `Baseline` or `User` origin, rate coverage, and
presence of dangerous overrides in machine-readable form. Internal sequence
numbers and closure `Debug` output remain private.

The snapshot supports readiness checks, tests, and operational verification; it
is not a serialization format for reconstructing an endpoint.

## 3. Public API stability

The project has not reached a production release. Compatibility shims do not
keep the current `ProductionPresetBuilder`, duplicate schema/HTTP builder
settings, or the old plugin-injection API alongside the new graph. After the
internal migration, the old surface is removed and compiler errors plus the
migration guide identify the equivalent new method.

## 4. Acceptance matrix

| ID | Expected |
| --- | --- |
| INJ-01 | A sugar method and its typed-injection equivalent produce the same effective configuration and response. |
| INJ-02 | The last call wins when the same single slot is set through sugar methods, injection, or both. |
| INJ-03 | Stage A and Stage B coexist, while reconfiguring the same stage replaces it. |
| INJ-04 | Multiple `inject` calls and namespace accessors edit the same draft graph. |
| INJ-05 | `push_*` preserves call order, `set_*` replaces the whole collection, and `clear_*` empties only the named collection. |
| INJ-06 | Final validation permits repair by later settings and refuses to return an endpoint when the final graph is inconsistent. |
| INJ-07 | A built endpoint exposes no configuration mutation method, and its configuration remains identical across clones. |
| INJ-08 | No public injection API uses `Any`, a string registry, or runtime downcasts. |
| INJ-09 | No API except a dangerous override can disable or remove a secure baseline limit. |
| INJ-10 | Effective configuration reports origins, rate coverage, and safety overrides without secrets, identities, or raw queries. |
| INJ-11 | Public rustdoc and IDE completion expose no second construction entry point such as `advanced`. |
| INJ-12 | No shim preserves the old preset or duplicate-builder API, and repository consumers use only the new surface. |

## 5. Consequences

### Positive

- Beginners and users who need fine-grained control use the same builder.
- Reading the method chain from top to bottom predicts the final values.
- Adding a sugar method does not duplicate implementation paths or security
  validation.

### Negative

- The implementation needs an internal model for the component graph and
  configuration origins.
- Last-write-wins does not turn an accidental override into a compile error, so
  effective-configuration inspection and tests are important.
- Existing builder users require one coordinated migration before release.

## 6. Rejected alternatives

- Add `Endpoint::advanced` as a separate entry point: this expands IDE and
  documentation choices and creates two contracts.
- Reject every duplicate setter: this makes composition helpers difficult to
  combine with application overrides.
- Use first-write-wins: nearby application settings can be ignored, defeating
  local reasoning.
- Expose an arbitrary service container: this sacrifices type safety,
  dependency boundaries, and security reviewability.
