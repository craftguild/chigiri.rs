# ADR-0055: Discoverable subscription configuration and lifecycle ownership

Status: Accepted / Implemented

Date: 2026-10-23

Implementation: SE-WP1 through SE-WP4 have local macOS arm64 evidence for
SE-01 through SE-08 under Beads `lab01-8ph`. The
validation record identifies the
numeric, managed-lifecycle, sizing, native consumer and focused quality checks.
Independent review decisions and correction verification are recorded in
Beads `lab01-8ph.4.1` and `lab01-8ph.4.2`. Non-local platforms, broad campaigns,
packaging and production capacity remain outside this acceptance.

Implementation goal: Prompt for a new agent.

Related: ADR-0004, ADR-0016, ADR-0017, ADR-0026, ADR-0027, ADR-0053, and ADR-0054.

Tracking: Beads `lab01-aw7` records the initial design publication;
`lab01-ssc` records the retained consumer inspection, and `lab01-9ez` records
the approved SDK-owned lifecycle revision. Implementation and review work
require separately scoped issues under the
[review workflow](../review-workflow.md).

## 1. Context and decision

A bounded POST SSE integration exercised typed source/event resolution, mapped
errors, reconnection and cancellation. That historical coverage did not qualify
WebSocket, multipart, browsers, production loads or arbitrary larger payloads.
Accepting a 1-MiB configuration is not evidence of delivering a 1-MiB event.

The useful friction is at the integration boundary: consumers must discover
private numeric bounds, infer missing configuration-error details, navigate a
hidden namespace for required shutdown, and assemble a server from test helpers.
The report does not justify removing resource protection or explicit ownership.

Keep ADR-0054's typed execution, protocols, quotas, and physical accounting.
Improve adoption through typed bound constants, narrowly allowlisted numeric
diagnostics, whole-configuration sizing guidance, and SDK-owned automatic
lifecycle management behind small official services and optional standard
server shutdown hooks. A runnable example must demonstrate that public surface.

The initial ADR-0055 draft proposed exposing existing driver/guard/reaper types
and teaching manual orchestration. Inspection of the retained consumer showed
that this would improve discovery without removing the integration burden.
This revision replaces that proposal. Automatic supervision and bounded shutdown
belong in the SDK, not generated code, application boilerplate, or duplicated
adapter state machines. It deliberately changes ADR-0054's application-polled
driver contract and the closed-failure HTTP boundary described below.

This document is self-contained. The sibling report is provenance, not a build,
test, or continuing acceptance dependency.

## 2. Preserved and amended contracts

- Preserve one Endpoint builder and one typed component graph. Sugar and
  `inject` replace the same complete values with final validation and existing
  origin/call-order semantics. Do not add a sizing builder or another policy DSL.
- Preserve every default, minimum, hard maximum, cross-setting rule, OR-based
  operational decision, and bounded reservation rule in ADR-0054 Sections 5
  and 9.3. Constants describe supported bounds, not a new override mechanism.
- Amend ADR-0017 Section 2.4 and ADR-0054's redacted configuration diagnostics
  only to permit the explicit numeric subscription-setting evidence below.
  Other diagnostic slots retain their existing redaction contract.
- Amend ADR-0004 and ADR-0054's lifecycle ownership: the SDK owns its managed
  execution, supervision and recovery; adapters only bridge server interfaces.
  Applications retain listeners, routes, origins, runtimes, transport admission,
  signals, unrelated application work and the decision to stop their server.
  Optional shutdown hooks do not make the SDK a server or signal handler.
- Replace mandatory manual driver/guard/process-drain calls and the proposed
  lifecycle re-exports with automatic managed services. Ordinary consumers need
  no hidden APIs, separate supervisor `run`, or separate shutdown-result await.
- Preserve complete resolver traits, domain values, one application-error
  mapper, fresh event contexts, Mock publication/rollback, and all security
  boundaries. No output DTO or domain-to-JSON round trip is introduced.

## 3. Public bounds and one source of truth

Expose associated `pub const` bounds on the existing public settings types.
Consumers should find them through ordinary rustdoc and IDE navigation without
importing a private validation module or copying a numeric literal.

The following names and values are the public contract. KiB and MiB mean
powers of 1024. Count/byte constants have type `usize`; time constants have
type `std::time::Duration`.

| Owner | Constants | Values |
| --- | --- | --- |
| `SubscriptionLimits` | `MIN_COUNT`, `MIN_BYTES`, `MIN_TIMEOUT` | 1, 1 byte, 1 nanosecond |
| `SubscriptionLimits` | `MAX_ACTIVE_SUBSCRIPTIONS`, `MAX_QUEUED_EVENTS` | 1,024 each |
| `SubscriptionLimits` | `MAX_QUEUED_BYTES`, `MAX_ENCODED_EVENT_BYTES` | 4 MiB, 1 MiB |
| `SubscriptionLimits` | `MAX_QUEUE_AGE`, `MAX_SEND_NO_PROGRESS_TIMEOUT`, `MAX_SETUP_TIMEOUT` | 60 seconds each |
| `SubscriptionLimits` | `MAX_EVENT_TIMEOUT` | 120 seconds |
| `SubscriptionLimits` | `MAX_ENDPOINT_ENCODED_BYTES` | 256 MiB |
| `SubscriptionLimits` | `MAX_PENDING_OPERATION_SETUPS` | 128 |
| `SubscriptionLimits` | `MAX_RETAINED_METADATA_BYTES` | 64 KiB |
| `SubscriptionLimits` | `MIN_HEARTBEAT_INTERVAL`, `MAX_HEARTBEAT_INTERVAL` | 1 second, 60 seconds |
| `SubscriptionProtocolSettings` | `MIN_COUNT` | 1 |
| `SubscriptionProtocolSettings` | `MAX_ACTIVE_OPERATIONS_PER_CONNECTION` | 128 |
| `SubscriptionProtocolSettings` | `MAX_WEBSOCKET_CONNECTIONS` | 512 |
| `SubscriptionProtocolSettings` | `MAX_PENDING_SETUPS_PER_CONNECTION` | 32 |

`MIN_COUNT` applies to count fields, `MIN_BYTES` to byte fields, and
`MIN_TIMEOUT` to queue age, send no-progress, setup, and event durations.
Heartbeat uses its separate minimum. Every bound is inclusive. Defaults remain
available through `Default`; do not introduce another default-value registry.

Public constants, final validation, metadata retention validation, and numeric
diagnostics use one authoritative definition per bound. Internal aliases may
reference it, but a second independently maintained literal table is not the
implementation. Private framing/cleanup constants and Mock-publication limits
are not made public or configurable by this ADR.

Document both required fit relationships next to the settings:

```text
max_encoded_event_bytes <= max_queued_bytes
max_encoded_event_bytes <= max_endpoint_encoded_bytes
```

Keep existing checked derived arithmetic and construction-time rejection.
The queue's byte and event limits are independent caps: construction does not
require `max_queued_bytes >= max_queued_events * max_encoded_event_bytes`.
Per-field hard maxima do not promise simultaneous capacity at all maxima.
Never clamp values or silently raise a related budget to make a setting fit.

## 4. Actionable, bounded configuration diagnostics

Retain `ConfigurationDiagnostic::slot()`, `origin()`, and `violation()` and the
existing `EndpointBuildError::InvalidConfiguration` path. Add a typed,
read-only `constraint()` accessor returning optional constraint evidence.
Use closed, allocation-free data with `usize` counts/bytes and `Duration`
values, not arbitrary strings, maps, serialized configuration, or callback
errors. Public evidence types and accessors must have normal visible rustdoc.

The evidence distinguishes:

- An inclusive range: supplied value, minimum, maximum, and its unit.
- A fit failure: the supplied event-byte value and the conflicting capacity
  slot, value, and origin. If both fits fail, report the queue fit first;
  otherwise report the endpoint fit. Retain a bounded single diagnostic.
- Checked multiplication overflow: the two operand slots, values, and origins,
  without evaluating or formatting an overflowed product.

Apply this evidence only to the numeric fields of `SubscriptionLimits` and
`SubscriptionProtocolSettings`. Range errors, including zero durations and
heartbeat values below one second, identify their actual permitted interval.
Keep existing violation classification and deterministic validation order.
Other slots return no new value evidence; they are not implicitly allowlisted.

`Display` and `Debug` render the same safe evidence as the typed accessor,
with explicit units and exact byte numbers. For example, an event-byte value
of 1,048,577 reports the supplied value and inclusive range 1 through
1,048,576 bytes, alongside the existing slot/origin/violation classification.
An individually valid 768-KiB event with a 256-KiB queue reports that fit failure
and both values, not a misleading event hard-maximum violation.

This is an explicit exception to the former no-values diagnostic rule, not a
general logging exception. Never expose a reason string, raw environment-variable
strings, credentials, headers, request metadata contents, documents, DTOs,
source errors, closures, or provider state. Do not echo the original textual
configuration input. These errors remain local construction diagnostics, not
new GraphQL response extensions, wire errors, or a serialized CLI format.

## 5. SDK-owned automatic lifecycle management

### 5.1 Ownership and the small public surface

The SDK facade owns the shared lifecycle state, supervised task handles,
registration, deadlines, cancellation, completion and bounded recovery for
managed GraphQL execution. Include finite Query/Mutation requests, subscription
setup and event work, and protocol operation ownership. The generated resolver
contract remains unchanged. Do not create another endpoint builder or put
runtime types in generated traits or the standard-library-only core.

Use the existing optional Tokio worker integration in the SDK. Official adapters
enable it and invoke the same managed implementation. Adapter responsibilities
are request/body/socket conversion, upgrade and disconnect notification, and
translation to native server interfaces. Admission, shutdown policy, timers and
recovery are not reimplemented for Axum versus Hyper or for each protocol.

Managed service construction starts required supervision fallibly in the
supported existing runtime. Missing/incompatible runtime or startup/resource
failure returns a construction error; do not return an unsupervised service,
panic, start another Tokio runtime, or open a listener. Endpoint/service clones
share the appropriate lifecycle and quota owners, not a supervisor per clone
or request. Keep bounded completion ownership and cancellation on last service
owner release; a spawned handle must never simply be discarded. Runtime loss
or supervisor failure closes admission and cancels affected work fail-closed.

A managed finite HTTP task owns the unique completion token immediately after
successful context preparation. Normal preparation disarms it before calling
the completion once. Cancellation, executor destruction or unwind drops the
in-flight execution before attempting the still-armed completion once with
`Failed` or `TimedOut`; an allocation-free bodyless callback input suffices for
this abandoned result. The same concrete task owns callbacks, panic payloads
and application destruction, never the native response waiter or process
reaper. A blocked callback/destructor retains physical request ownership through
actual join. A callback panic cannot cause a retry, unmasked response or false
successful drain. `Encoded` describes bounded preparation before callback
execution, not callback CPU time or client receipt. Preserve the existing
finite handler's compatibility behavior outside managed service activation.
If future or panic-payload disposal cannot establish release, retain the
original reachable poisoned worker record and physical owners under the
existing fail-closed recovery policy. A native join alone does not release
capacity after failed disposal, and a task-side completion flag is not a join
witness.

The following public API shape is exercised by the maintained native-hook
fixture in `crates/testkit/tests/managed_shutdown.rs`. The runnable
[Query/POST-SSE application](../../examples/subscriptions/README.md) uses the
same service and standard native hook.
`graphql` is the application's already-built `chigiri::Endpoint`; `listener`,
`shutdown_signal` and other routes belong to the application:

```rust,ignore
let app = axum::Router::new()
    .route_service("/graphql", chigiri_axum::service(graphql.clone())?);

axum::serve(listener, app)
    .with_graceful_shutdown(graphql.shutdown_on(
        shutdown_signal(),
        chigiri::ShutdownOptions::graceful(),
    ))
    .await?;
```

Route registration alone is a supported minimal integration. Hyper uses the
same SDK-managed service/lifecycle through its native service and connection
interfaces. Neither adapter returns a tuple of manually polled lifecycle owners
as the normal integration contract. WebSocket upgrade/session supervision also
belongs to SDK-managed ownership rather than an application driver per upgrade.
Internal bridge SPIs may remain hidden; do not promote driver/guard/reaper types
merely to make examples compile. No server extension trait, `with_chigiri_*`
method, `graphql.run(server, ...)`, or Chigiri-owned server is introduced.

### 5.2 Optional shutdown policy

Provide SDK-owned `ShutdownOptions::immediate()` and `::graceful()` values;
the latter uses a 30-second `max_duration`. Provide a fallible typed duration
override, illustrated as `::graceful_for(Duration::from_secs(10))?`. Reject zero,
unbounded sentinels and unschedulable deadlines before installing the hook.
These are lifecycle options, not mutable schema/execution configuration or
another settings registry. `shutdown_on` accepts the options and an owned
`Future<Output = ()> + Send + 'static` supplied by the application; it returns
a hook compatible with the native server shutdown method.

| Integration | SDK behavior |
| --- | --- |
| No hook | No coordinated drain guarantee or knowledge of server shutdown start. Last service-owner Drop requests cancellation; runtime/process termination may end work abruptly. |
| Hook with Immediate | On notification, close new GraphQL admission and request cancellation immediately; do not wait for work to finish. |
| Hook with Graceful | On notification, close new GraphQL admission, stop subscription demand, and wait for eligible in-flight work within one deadline. Cancel remaining work when the deadline expires. |

No hook is immediate-like fallback behavior, not a promise to terminate the
server or detect its shutdown start. It does not weaken masking, quotas or
physical ownership during ordinary operation. `Immediate` is a cancellation
request, not thread preemption, rollback of committed application writes, or
proof of completion. The SDK never calls process exit to implement it.

Graceful waits for admitted finite requests to finish their SDK execution and
response preparation. Finite admission is elected after operation/media
selection and before finite context construction, ordered with shutdown. Input
still receiving, decoding or resolving an unselected operation is cancelled
without starting a new context. A finite rejection already prepared before
notification may finish. Existing finite handlers outside managed activation
retain their original behavior. Graceful does not wait for a subscription source to complete
naturally. Stop further source demand and cancel a pending next-item wait;
allow already executing/encoding or sending event work to finish within the
remaining limits. Do not flush the unstarted event backlog, start new event
executions, or transmit a newer result past skipped older queued results.
An executing result that cannot be emitted without violating that order is
discarded on termination, not sent out of order. Already partially sent frames
remain immutable and non-evictable while their bounded send is allowed to finish.
Shutdown abandonment is cancellation, not a new DropOldest accounting category.

Take one monotonic deadline at the accepted shutdown notification. Every drain,
send and active cleanup step uses its remaining time; do not add another full
cleanup wait after `max_duration`. Existing shorter setup, event, request and
send limits still apply. Repeated notifications join the existing transition
and cannot reset its policy/deadline. At expiry, revoke emission, cancel and
retain any unjoined physical owners under existing bounded recovery; do not
release charges early or report them as drained. Blocking application code
cannot be forcibly stopped or guaranteed to destruct within this duration.

### 5.3 Native server hooks and the SDK-first ordering

Use standard server APIs rather than extensions over concrete server types.
For the Axum shape above, the hook first observes the application signal,
performs the SDK's chosen stop/drain, and only then completes the future that
allows Axum's native server shutdown to begin. Immediate completes that hook
after requesting cancellation, not after joining all workers. Graceful
completes it after scoped drain or deadline/failure; no extra application await
or wrapper around the server future is required.

During SDK drain, reject new requests/subscriptions for that endpoint with a
bounded safe unavailable outcome and never initialize a source. The listener
can still accept TCP connections and other routes can still serve. Only the
application/server can stop those; document this chosen ordering rather than
claiming simultaneous server-wide admission closure. Without a hook, native
server graceful shutdown can wait indefinitely for long-lived subscriptions;
the SDK cannot infer the missing notification from route registration.

`max_duration` bounds SDK waiting, not total server shutdown, other routes,
network delivery or arbitrary application destructors. After the SDK hook
completes, the server applies its own connection/write/drain behavior. Wire
buffers already handed to the server remain its responsibility. Keep
subscriptions isolated from unrelated endpoints and application runtimes.
Process-global recovery stays internal and is not shut down by one endpoint's
hook; a process-lifetime reaper is not a per-endpoint worker to join. Ordinary
users need no manual final process-drain call. Do not describe scoped Graceful
as a guarantee that every process thread has joined before runtime destruction.

### 5.4 Route-compatible failures and observable outcomes

The managed Axum service must fit `Router::route_service`, with infrastructure
failures converted in the SDK before the adapter returns a response. This
explicitly replaces ADR-0054's requirement to propagate every closed initial
failure to the connection service without committing a response head.
Preserve existing finite request/setup rejections and their statuses; a closed
internal preparation/attachment failure uses a bounded masked finite HTTP 500,
never an empty successful stream or raw error. Reuse an already charged safe
finite failure when available; otherwise return a bodyless HTTP 500 without
claiming a GraphQL/streaming body or allocating another encoded payload. Do not
retry source/hooks/completion, expose raw errors, or create an uncharged encoded
fallback, new emergency pool or adapter-local error policy. Keep startup errors
fallible.
After response commit, use protocol termination or a body/connection error;
do not replace the committed status or turn abnormal termination into clean EOF.

Provide a safe typed read-only shutdown result through the SDK and existing
observation integration. Distinguish successful scoped drain, cancellation
requested, deadline expiry and infrastructure failure. Counts/classifications
may be exposed, never credentials, documents, source values or raw errors.
Outcome inspection is optional, not another required shutdown await. Preserve
the original timeout/failure outcome if physical cleanup completes later.
A completed hook is not itself proof of a successful drain or peer receipt.

## 6. Whole-configuration sizing guidance

Extend the subscription walkthrough, linked from Rust integration and settings
rustdoc. This is documentation plus checked examples, not a new sizing engine,
allocator, estimator API, or automatically tuned configuration.

The guide must distinguish:

- Rust/domain heap size and raw text byte lengths;
- the selected GraphQL result, aliases, errors, and application extensions;
- UTF-8 JSON serialization and escaping;
- Chigiri delivery metadata, including the reserved 16-byte counter value slot;
- the supported profile's application framing: SSE lines, multipart boundaries
  and part headers, or the WebSocket GraphQL envelope and operation ID; and
- transport-library WebSocket frame headers, HTTP chunk/HTTP2 framing, TLS,
  socket/proxy/client buffers, and upstream domain queues, which are outside
  the encoded-event budget.

`max_encoded_event_bytes` caps the complete Chigiri-encoded profile event, not
just its DTO or inner JSON. Do not add the included profile framing twice.
An event fitting one transport may not fit another. Do not claim that a fixed
multiplier on raw DTO bytes bounds every result; test representative maximum
selections, escaping, errors, extensions, and operation IDs.

Explain reservation and concurrency with the existing accounting:

- Per-subscription byte capacity includes queued and in-flight event entries;
  its event-count cap excludes the one in-flight entry.
- One maximum-event reservation is acquired before source demand, then shrunk
  to actual encoded bytes. It is not another independently queued event or an
  uncharged candidate, and transfers must not double-count its token.
- For planning, `S * (Q + E)` is a conservative event-storage allowance for
  `S` simultaneously active subscriptions, local byte cap `Q`, and maximum
  candidate reservation `E`. It is not a required build-time inequality or a
  complete endpoint-memory bound. Pending initial responses, control frames,
  heartbeats, terminal messages, and physical retained owners also need room
  in the shared encoded budget. Retained event bytes already charged within
  `Q` must not be added a second time merely because cleanup is pending.
- A valid configuration can still pause or terminate under shared pressure.
  Giving every subscription its configured maximum simultaneously is a
  workload/capacity decision, not promised by individual hard bounds.

Include one default example and one larger-event example based on the report's
768-KiB event / 2-MiB queue configuration. Show the active-count and endpoint
budget choices explicitly, explain which queued/in-flight concurrency they
target, and reserve headroom for control/setup rather than silently borrowing
it. Verify the example's exact arithmetic and its representative escaped
payload with the production bounded encoder. Configuration acceptance alone
is not payload-delivery or production-capacity evidence.

## 7. A small, maintained adoption example

Add a discoverable, version-controlled application consumer, linked from the
walkthrough, using complete generated ABI 8 traits, a finite Query and a bounded
owned subscription source. Exercise both through one application-owned HTTP
listener using the official Axum streaming service and POST SSE. Show the
small managed `Router::route_service` integration rather than a hand-built
Hyper accept loop. The constructor is fallible, while the routed service
uses the native Router-compatible response boundary from Section 5.4.
Keep the existing finite handler valid for applications without streaming.

The example uses only documented public application APIs and owns routing,
origin/admission checks, signals, listener and runtime; the SDK owns managed
supervision. Show a no-hook minimal route and a standard shutdown hook using
the 30-second Graceful default plus a typed override and Immediate option.
Include wrong-path/wrong-origin rejection before setup, source cancellation on
disconnect, automatic supervisor failure handling, and both successful and
timed-out shutdown outcomes. Never require hidden imports, manual driver polls,
per-upgrade drivers, process-drain calls or a new server extension method.

Explain source setup, event-to-root mapping and selected object-field resolver
roles together. Keep application catch-up/replay/watch-race logic separate
from the minimal framework connection. Show owned source handles and cheap
shared domain backing where appropriate; a pass-through event need not deep
clone an entire batch. Do not copy the disposable proof's stderr/stdin control,
late-only connection-task joins or success-path-only cleanup as production
requirements. Existing routing/admission protections still apply.

Document the generated contract's standalone Cargo workspace and show a
compatible consumer workspace layout with explicit exclusion where needed.
Explain how to check producer revision/build identity, generated ABI, and exact
ownership before compiling the consumer. Compilation alone does not establish
generator freshness. Do not instruct users to edit verified generated output.
This is a checked example, not a new scaffold command or generated server.

Generated-package license metadata and runtime dependency version inputs are
separate packaging work. Record them as follow-up candidates, not subscription
runtime failures, and do not add metadata/manifest formats or dependency
allowlists in this ADR. A future packaging change must retain protected output.

## 8. Compatibility and exclusions

This is a Rust facade/bridge adoption and lifecycle change plus a narrow
diagnostic-contract amendment. It supersedes mandatory manual supervisor
ownership, proposed public lifecycle re-exports, and the initial closed-failure
connection boundary; it is not merely an additive documentation change.
Preserve generated ABI 8 and all current Mock project, bundle, host API,
reference, log, management, snapshot, and CLI formats.
No canonical SDK regeneration or identity cutover is required solely to add
public constants, managed SDK lifecycle, and typed construction diagnostics.
The example uses the normal protected generator and ownership rules for its
own contract.

Migrate official bridges, maintained consumers and affected internal users,
including Mock's server where it uses this bridge, to the same managed owner.
Do not retain duplicate manual and automatic supervision or a second published
manual-driver integration mode. Internal low-level SPIs may remain for bridge
implementation and focused tests. Retain finite-handler behavior, transport
profiles, source/error semantics and existing Mock log/transaction meanings;
no new Mock configuration/CLI shutdown feature is part of this revision.

Do not raise the 1-MiB event ceiling or any other hard maximum, add unbounded
values, alter backpressure/DropOldest, or change encoded-byte accounting.
Permit owned automatic supervision and the Router-compatible failure mapping
explicitly specified above, not arbitrary detached work or implicit OS signal
handlers. Do not add transport profiles, a Rust server product, server extension
traits, mandatory `run`/join boilerplate, ACK, replay, reconnection, history,
a client SDK, or internal delivery fields in application SDL. Applications
retain these domain and deployment responsibilities.

## 9. Implementation boundaries and acceptance

Implement as small named responsibilities: authoritative bounds and diagnostic
evidence; common managed lifecycle and native service/hook glue; then guidance
and the executable consumer. Update maintained tests that assert the old
no-values diagnostic rule only for
the explicitly amended slots. Tests use this ADR and checked-in fixtures, never
Beads verdicts or the sibling report's prose/results as their oracle.

| ID | Required evidence |
| --- | --- |
| SE-01 | Public constants cover every configurable Rust subscription/protocol field, match ADR-0054, and are the definitions used by range/retention validation. Default values and accepted/rejected boundaries remain unchanged. |
| SE-02 | Sugar/inject retain identical effective values/origins and invalid-graph behavior. Both fit failures, zero/min/max/over-max, heartbeat minimum, and derived overflow have typed numeric evidence with correct units and deterministic classification. |
| SE-03 | Accessor, Debug and Display agree on numeric evidence; non-allowlisted slots retain redaction. Secret markers in reasons/providers/request material never appear in these diagnostics. |
| SE-04 | External MSRV consumers register a managed Router service and optional standard shutdown hook without manual lifecycle owners, hidden APIs or server extensions. Both official bridges use the same SDK owner; missing runtime/startup errors fail construction, and neutral/default dependencies remain Tokio-free. |
| SE-05 | Automatic startup, no-hook last-owner cancellation, Immediate, default/overridden Graceful, repeated notifications, races and supervisor failure remain bounded and fail-closed. Cover finite Query/Mutation, subscription setup, active versus waiting events, send/cleanup deadlines, clone sharing, endpoint isolation and retained physical ownership. The native hook signals server stop only after SDK drain/timeout; isolate irreversible process fixtures. |
| SE-06 | The guide's default and larger-event arithmetic is checked. Bounded production-encoder fixtures include escaping, metadata and profile framing, exact fit and overflow. No new total-memory or delivery guarantee is asserted. |
| SE-07 | The example executes finite Query and POST SSE through a native Router, rejects wrong routes/origins before source setup, cancels on disconnect, and uses no-hook and standard-hook shutdown without manual drivers or extra await. Verify SDK-first order, other-route availability during drain, safe pre-stream failures, abnormal body termination, successful/expired outcomes, workspace layout and producer identity. |
| SE-08 | Focused affected tests, MSRV/stable compilation, Clippy, rustdoc, formatting, implementation rules and diff checks pass; source-bound evidence and independent review are recorded in Beads. |

Use bounded local macOS validation and existing deterministic fixtures. Do not
require a full-product validator, remote Linux, cross/distribution builds,
browser qualification or load/soak/fuzz campaigns. Compile both bridges and
exercise the changed managed HTTP/upgrade lifecycle across all three existing
protocols with small fixtures; unchanged framing/property tests need not all
be repeated. Do not retire old integration fixtures without replacing their
ownership/error/cleanup assertions. Report every unexecuted scope honestly;
this assignment does not change support tiers or qualify production capacity.

## 10. Consequences

Consumers can discover legal settings, diagnose mistakes and attach ordinary
routes without private-source inspection or supervisor assembly. Optional
standard hooks add bounded SDK shutdown without a Chigiri server wrapper.
Ownership remains explicit inside the SDK rather than delegated to beginners.

Public bounds, managed activation and shutdown hooks become API obligations.
Automatic startup needs a supported runtime, numeric diagnostics need a narrow
disclosure boundary, and lifecycle migration needs race/physical-owner tests.
SDK-first drain can delay server-wide admission closure; omitting the hook
provides no coordinated completion guarantee. These explicit tradeoffs are
preferable to server extension coupling, detached tasks, removed caps or a
second manual integration mode. The
validation record identifies the
implemented contract's focused local macOS arm64 evidence. Beads holds the
independent review decisions and correction verification. Non-local platforms
and production capacity remain unqualified.
