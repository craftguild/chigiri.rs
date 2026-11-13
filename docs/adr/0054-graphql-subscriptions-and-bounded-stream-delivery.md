# ADR-0054: GraphQL subscriptions and bounded stream delivery

Status: Accepted / Implemented

Date: 2026-10-21

Decision finalized: 2026-10-22, including the initial safety defaults,
configuration hard maxima, and fallible construction policy in Section 5.

Implementation: Local implementation and focused acceptance completed on
2026-10-23 under `lab01-uid`. The current source implements generated ABI 8, typed Rust
and Mock sources, bounded delivery, and all three profiles through both official
bridges. The validation record
identifies actual executed scopes, final canonical checks and source identities.
Both whole-goal reviews (`lab01-uid.4.9` and `lab01-uid.4.10`) accepted the local
implementation. No release, non-local qualification or production capacity is claimed.

Tracking: Beads issue `lab01-q2m` covers the original documentation publication;
`lab01-b2d` records the approved defaults and construction-policy amendment.
Implementation and independent reviews require their own scoped Beads issues
under the [review workflow](../review-workflow.md).

2026-10-24 implemented follow-up:
[ADR-0055](0055-subscription-configuration-and-lifecycle-ergonomics.md) defines
public bound constants, narrowly allowlisted numeric configuration diagnostics,
SDK-owned automatic lifecycle management, thin adapter services, sizing guidance
and an adoption example. It replaces application-polled driver/guard/reaper
assembly with managed activation and optional standard server shutdown hooks:
no coordinated drain without a hook, Immediate cancellation or configurable
30-second-default Graceful when connected, and SDK-first drain before native
server shutdown. Section 9.3's mandatory manual driving/process-drain calls and
non-Router initial-failure boundary are superseded at that cutover. Safe finite
initial HTTP errors replace connection-only failures where specified; partial
streams never become successful completion. Resource bounds, physical charges,
masking, source/hook once points, protocols and version identities remain.
This follow-up is Accepted / Implemented with local macOS evidence in the
ADR-0055 validation record.
The earlier ADR-0054 acceptance retains its recorded source scope; non-local
platforms and production capacity remain unqualified.

## 1. Context and decision

The application-integration experiment and subsequent design discussion called
for typed, push-oriented GraphQL APIs without making Chigiri an application
server or requiring domain values to pass through JSON. ADR-0053 established
ordinary complete resolver traits, domain associated types, and one application
error mapper. Subscriptions extend that model rather than adding a projection
DSL, output DTOs, or an untyped resolver registry.

Long-lived WebSocket, SSE, and multipart responses all retain resources. Their
cost depends on active subscriptions, event execution, queued bytes, and slow
consumers, not just transport names. WebSocket may share the HTTP listener and
port; no separate listening port is a framework requirement. HTTP/2 stream
multiplexing does not remove per-subscription state or event queues.

Implement subscriptions for both ordinary Rust consumers and Luau Mock. Keep
execution and delivery policy transport-neutral, with official Hyper/Axum
bridges for WebSocket, SSE, and HTTP multipart. Prepare fresh application
context for every event, accepting that cost in exchange for explicit ownership,
authentication opportunities, and isolation. Prefer correctness and safe,
understandable production integration over throughput optimization.

Use bounded backpressure without silent dropping by default. Permit a server to
select `DropOldest` for an individual subscription. Report Chigiri-owned drops
through common response metadata, not application SDL fields.

## 2. Supersession and retained boundaries

The following changes take effect only at a coordinated implementation cutover:

| Existing contract | Extension |
| --- | --- |
| ADR-0001/0002/0005 and the current parse/reject boundary | Validate and execute subscriptions, retaining typed arguments, scalar/enum/abstract output rules, selection, and normal null propagation. |
| [ADR-0004](0004-http-adapters.md) | Extend the finite HTTP response boundary with neutral streaming and upgrade integration. Preserve existing finite Query/Mutation behavior and application-owned listeners, routing, TLS, runtime, and shutdown. |
| [ADR-0016](0016-single-endpoint-builder-and-safe-baseline.md) / [ADR-0017](0017-typed-component-injection-and-override-semantics.md) | Add subscription settings to the same safe endpoint draft, typed injection, effective configuration, and final validation. No second endpoint builder. |
| ADR-0026/0027/0028 and [ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) | Extend generated contracts and private adapters for typed source initialization. Retain ordinary traits, one mapper, semantic generated layout, ownership, exact checks, and protected regeneration. |
| ADR-0029/0030/0031/0032/0036/0039/0045 | Extend Mock with explicit, bounded, post-commit publication and read-only event execution. Preserve the sandbox, isolated workers, request atomicity, masking, and private-output rules. |
| ADR-0046/0051/0052 | Give each Mock event a fresh clock/state view and preserve completion-free validation, adopted-schema authority, local codegen, and portable export. |

`chigiri_core` remains standard-library-only and does not own tasks, timers,
channels, sockets, or an async runtime. Public neutral contracts expose no
Hyper, Axum, Tokio, or WebSocket-library types. Standard ecosystem stream
integration belongs outside that core boundary; do not build a new stream DSL.
Protocol libraries must be reviewed public releases, not vendored or patched
upstream implementations.

The Rust application owns transport selection, routes, connection admission,
HTTP version, TLS, proxy configuration, and shutdown/drain. Subscription quotas
inside Chigiri do not replace application TCP connection limits. The existing
Mock CLI still owns its loopback server; this does not authorize a new general
Rust server distribution or external networking from Luau.

## 3. Typed source initialization and event execution

Apply the September 2025 subscription validation rules, including the single
collected root field and its introspection/directive restrictions. Preserve
aliases, fragments, coercion, and field selection. Validate and coerce the
operation before establishing its source; do not rerun source initialization
for each event.

Generate a complete source-resolver contract whose successful value is an owned,
`Send + 'static` stream of typed `Send + Sync + 'static` root values, with
application errors. An outer
error represents failure to establish the source; an error from the source
terminates that subscription. Both use the assembly's existing application
error mapper exactly once. Ordinary event field failures retain normal
GraphQL field-error handling instead of becoming transport failures.

`SchemaTypes` continues to retain domain backing values. Source items have the
SDL field's output shape, including nullability and lists; missing or wrong
implementations fail compilation. Each item triggers ordinary selected-field
resolution. No intermediate output-object DTO or JSON round trip is required.
Retain correct ordinary object-field behavior if a root type is also used as a
nested object; a stream return must not replace that single-value contract.

The following is an API-shape illustration, not currently available code or a
complete consumer. `Presence`, the service, and context are application-owned:

```rust,ignore
impl generated::SubscriptionResolver<AppContext, Types, AppError>
    for SubscriptionFields
{
    type PresenceEvent = Presence;

    async fn subscribe_presence<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        args: &'a generated::SubscriptionPresenceArgs,
    ) -> Result<chigiri::SubscriptionSource<Self::PresenceEvent, AppError>, AppError> {
        let source = ctx.context().presence.watch(args.room_id.clone()).await?;
        Ok(chigiri::SubscriptionSource::new(source)
            .with_overflow(chigiri::SubscriptionOverflow::DropOldest))
    }
}
```

Omitting the explicit overflow selection retains non-dropping behavior. The
source must own any shared service handles it needs rather than borrowing the
setup context or request buffers for the subscription lifetime. Keep delivery
policy in the API layer, not as a dependency imposed on domain services.
This abbreviated illustration omits the required event resolver and any
ordinary nested-field resolver; Section 9.1 provides the complete method-role
and ownership example.

Preserve source order among retained events. Start with bounded sequential
event execution per subscription; do not create an unbounded task for every
event or allow a slow subscriber to block unrelated subscriptions. No automatic
source sharing, domain acknowledgement, read marker, or membership mutation is
introduced.

## 4. Fresh context and phase ownership

Prepare one setup context to authorize the subscription and establish its
source. Release it after setup; do not retain it as the event context. Before
executing each event, invoke context preparation again and await a fresh
`PreparedContext<C, S>`. Applications may deliberately share pools and services
through `Arc`, but Chigiri must not reuse the preceding event's context, cache,
principal, response state, execution identity, or budget.

Retain a bounded, owned, immutable request-metadata snapshot for re-preparation.
It belongs to the subscriber, not an event publisher. Expose setup/event phase
identity explicitly to preparation and observability. Per-event preparation
can revalidate the originally supplied credentials against current application
state. It does not refresh credentials or guarantee revocation without an
application check; supplying different credentials requires a new subscription.

Give setup and each event their own bounded preparation/execution deadlines.
Do not reuse the original finite request's execution timeout as an accidental
deadline for the entire subscription. Event lifetimes and queue/send limits are
separate. Release the previous event's request-owned state before continuing;
keep the subscription operation, variables, counters, and source separately.

Run applicable authorization, operation/field policy, and observers for event
execution without redoing the transport handshake or registering the source.
This includes authorization for the subscription root and its arguments, not
only nested output fields. Checks written solely inside source initialization
do not rerun automatically: applications must place changing permissions in
fresh preparation or per-event policy. Revalidation at event execution is not
instantaneous push revocation or authentication of each transmitted byte.
Use existing masking, error IDs, aliases, locations, and paths. Response headers
and the HTTP response hook belong to initial response establishment and cannot
be reissued after streaming starts. Per-event response state is not a new way
to mutate already-sent headers.

| Failure phase | Required behavior |
| --- | --- |
| Pre-stream decode, validation, admission, or setup preparation | Reject before establishing the stream, using the applicable HTTP or operation-level protocol error. |
| Ordinary selected field in an event | Produce the normal event result: Rust retains partial data/null bubbling; Mock retains its all-or-nothing event result. No duplicate application-error mapping. |
| Event context preparation rejection, internal failure, or timeout | Do not execute or emit that event's data; terminate the affected subscription and cancel pending work. |
| Source error, unrecoverable limit, or internal execution failure | Terminate with a safe reason where the protocol and writer still permit it. Do not report normal completion as proof of success. |
| Disconnect, unsubscribe, or shutdown | Cancel the source, preparation/execution futures, timers, and queued work; release owned permits and workers. |

After a stream starts, its HTTP status cannot be replaced. Translate terminal
failures through the selected protocol without exposing raw preparation bodies,
headers, credentials, or application error text. A blocked/broken connection
cannot be promised a final error frame; bounded cleanup must still complete.
Cancellation remains cooperative and cannot preempt blocking application code.

## 5. One configuration graph and portable limits

Subscription sugar methods delegate to the same leaf operations as
`inject(|components| components.subscriptions()...)`. Preserve synchronous
injection, call-order last-write-wins, final cross-component validation, closed
redacted diagnostics, immutable built endpoints, and baseline/effective/origin
inspection. No service locator, arbitrary condition DSL, or second builder is
introduced. Ordinary settings cannot disable safeguards through zero, `None`,
or an unbounded sentinel.

### Accepted initial defaults and hard maxima

The following values are accepted design defaults, not measured capacity or
answers to an application's non-functional requirements. They provide a compact,
bounded starting point when no tuning is supplied. Applications tune within
the supported range for their own workloads; no production-load campaign is
required to adopt this baseline. These are target contracts, not settings
already available in the current executable. KiB and MiB use powers of 1024.

| Setting and scope | Default | Configuration hard maximum |
| --- | --- | --- |
| Active subscriptions per Endpoint | 128 | 1,024 |
| Active operations per multiplexed protocol connection, including Query, Mutation, and Subscription | 16 | 128 |
| Queued event results per subscription | 32 | 1,024 |
| Queued encoded bytes per subscription | 256 KiB | 4 MiB |
| Single encoded event result, including delivery metadata | 64 KiB | 1 MiB |
| Oldest queued event residence time | 10 seconds | 60 seconds |
| No-progress deadline while a send is pending | 10 seconds | 60 seconds |
| Subscription setup, including its preparation and source initialization | 10 seconds | 60 seconds |
| Each event's preparation and execution combined | 30 seconds | 120 seconds |

Every configurable limit above must be positive and no greater than its hard
maximum; equality is allowed. Defaults are not lower bounds. The connection
quota applies to protocols that multiplex operations, not to application-owned
TCP admission or an implicit quota on every HTTP/2 connection. The setup and
event deadlines are independent bounded phases, not the subscription's lifetime.
Existing shorter phase/VM limits remain effective; these values do not extend
them. An idle source does not consume an event-execution or send-stall deadline.

The following additional initial defaults are also accepted:

| Guardrail and scope | Default |
| --- | --- |
| Total Chigiri-owned encoded buffers per Endpoint, shared by its clones | 64 MiB |
| Concurrent pending operation setups per Endpoint | 16 |
| Retained subscriber request metadata per subscription | 16 KiB |
| Pending publications per complete Mock Mutation request | 32 |
| Pending publication payload bytes per complete Mock Mutation request | 1 MiB |
| Heartbeat interval, where supported by the selected protocol profile | 15 seconds |

SU-WP0 must complete the finite hard ceilings and exact accounting for these
additional guardrails, plus protocol framing/control, handshake, and cleanup
bounds, before product implementation. Preserve the accepted defaults while
doing so; changing them or the first table's hard maxima requires an explicit
design amendment, not a separate choice by each bridge. Prefer grouped settings
and private implementation bounds over a public setter for every buffer.

At the defaults, full subscription queues account for at most
`128 * 256 KiB = 32 MiB` of encoded event results. Encoding, in-flight writes,
and protocol control consume additional space and share the 64 MiB encoded
buffer budget; reserve capacity before allocating or retaining those buffers.
Per-setting maxima do not promise all maxima can be reached simultaneously.
Keep shared pressure in the common policy evaluator, preserve partial/control
messages, and never evict another subscription's events to make room for the
affected one. If that subscription cannot stay within the shared budget using
its allowed recovery policy and finite waiting, terminate it safely. Mock
delivery exhaustion after commit must never roll back the Mutation. Arbitrary
domain heaps, unencoded source values, and OS/network buffers remain outside
this encoded-byte guarantee; the Mock candidate has its separate request bounds.

### Configuration rejection versus operational limits

Rust type mismatches are compile-time failures. Configuration values may come
from environment variables, files, or application computation and are validated
at runtime during fallible Endpoint construction. A hard-maximum violation,
zero/unbounded value, arithmetic overflow, or inconsistent combination returns
`Err` before the new Endpoint can serve requests. Do not panic, silently clamp,
log-and-accept, or defer invalid configuration until a subscription is running.
Preserve closed, redacted diagnostics identifying the affected setting/rule.
An invalid construction does not mutate an existing immutable Endpoint.

For example, an active-subscription value of 1,025 has the correct Rust numeric
type but must be rejected at construction because its hard maximum is 1,024.
Validate combinations as well as individual values, including whether a single
event can fit the configured queue/staging budgets. A valid built Endpoint
enforces its effective values, not merely the configuration hard maxima:
actual load reaching a limit follows the rejection, backpressure, eviction,
or termination policies below rather than crashing the application.

### Configuration API shape and delivery policy

This example uses the accepted defaults. API spellings remain illustrative
until SU-WP0; the snippet is not a presently supported API:

```rust,ignore
.inject(|components| {
    components.subscriptions().limits(SubscriptionLimits {
        max_queued_events: 32,
        max_queued_bytes: 256 * 1024,
        max_queue_age: Duration::from_secs(10),
        max_active_subscriptions: 128,
        ..Default::default()
    });
})
```

The corresponding builder sugar sets exactly the same slot. A limits value
replaces the entire limits setting; `..Default::default()` is not a partial
patch of a previous setting. Endpoint clones share the same quota owner, not
independent copies of the configured capacity. Separately built endpoints and
processes do not silently share global quotas.

Evaluate applicable violations with OR semantics: any violation requires a
decision. Separate pure, common limit evaluation from policy resolution and
protocol I/O. Adding a condition must not require reimplementing the decision
in each bridge or writing OS-specific product logic.

| Condition and scope | Default non-dropping policy | Explicit `DropOldest` |
| --- | --- | --- |
| Queued event count or total encoded bytes, per subscription | Stop requesting further source items; terminate if finite waiting/retention limits cannot be met. | Evict oldest wholly unsent event results until all applicable capacity conditions permit admission. |
| Oldest queued event residence time, per subscription | Terminate rather than silently skip expired events. | Evict expired wholly unsent events and recheck all conditions. |
| Send has made no progress within its deadline | Terminate the affected subscription. | Same; eviction cannot repair a stalled writer. |
| A single encoded event exceeds its limit | Terminate. | Same; evicting other events cannot make this event valid. |
| Active subscription count, per endpoint and applicable connection quota | Reject the new subscription atomically; retain existing subscriptions. | Same; do not evict another subscription. |

After eviction, re-evaluate every applicable condition. Do not resume because
only one violation disappeared. An unrecoverable violation wins over an
otherwise recoverable queue overflow. Count admission and release must be
atomic under concurrent starts/cancellation, with no leaked or double releases.

Except for connection/protocol failures that necessarily affect the transport,
terminate the subscription, not every operation sharing its WebSocket. A
non-reading shared socket may ultimately require connection-level closure;
do not claim isolation after that transport itself is unusable.

The shared policy uses monotonic time and neutral deadline/timer SPI. Recheck
on admission, enqueue, dequeue, and applicable timer expiry, even when no new
event arrives. Idle sources are not stalled writers. Heartbeats do not extend
an old data event's residence deadline. Host clock substitution/timeleap cannot
extend resource deadlines.

Byte accounting covers complete encoded event results retained by Chigiri,
including its delivery metadata. Account for separately bounded in-flight
encoding/writing and control frames; never allocate an unbounded candidate and
check only afterward. These are not total-memory guarantees for arbitrary
domain objects, upstream queues, OS buffers, or intermediaries. Document those
application boundaries and cap internal staging/task metadata separately.

`DropOldest` is an explicit server-side, per-subscription semantic choice. It
cannot disable any hard cap and is not a GraphQL argument or client extension.
Non-dropping backpressure is not durability or guaranteed receipt: a source
unable to respect bounded demand must fail/terminate rather than silently lose
events under that policy.

## 6. Common delivery metadata

Include the following map in every emitted subscription execution result,
inside the transport's ordinary result envelope:

```json
{
  "data": { "presence": { "online": true } },
  "extensions": {
    "chigiri": {
      "delivery": { "droppedTotal": 3 }
    }
  }
}
```

- `null`: `DropOldest` is disabled.
- `0`: `DropOldest` is enabled and no event has been discarded.
- A positive integer: cumulative Chigiri-owned discarded event results for
  this subscription. A new subscription starts a new counter.

The path `extensions.chigiri.delivery.droppedTotal` is framework-owned. Final
event encoding creates missing `chigiri` and `delivery` objects, preserves every
non-conflicting application key in those objects, and overwrites any application
value at the reserved `droppedTotal` leaf with the canonical counter. If either
existing prefix is not an object, the event terminates with a masked internal
extension-collision error instead of destroying the value or emitting spoofed
metadata. This merge occurs before the capped final encoding, so every inserted
key/value byte is included in the single-event, subscription, and endpoint byte
reservations. Fixtures cover absent paths, preserved sibling keys, a forged
leaf, and non-object collisions at both prefixes.

Count each wholly unsent event result removed for the selected overflow/expiry
policy once, not protocol frames or bytes. Preserve the latest cumulative count
at emission, even if an earlier queued result carrying metadata was itself
evicted. Counters must not wrap or silently lose integer precision. Terminal
failures, control messages, heartbeats, and partially transmitted results are
not discardable event entries.

The count says nothing about upstream loss, network failure, reconnect gaps,
client receipt/application, or durable history. It is not an acknowledgement
or cursor. If no subsequent event can be delivered, the client may never
observe the final count. Retain bounded server-side termination/drop observation
without promising a final message on a broken connection.

Do not add Chigiri-internal loss fields to application SDL. Document how an
advanced client reads response extensions, including a small Apollo Link
recipe. Ordinary Apollo multipart subscription data remains the simple path;
do not promise that `useSubscription` exposes arbitrary response extensions.
Verify the recipe against a recorded client version. An additional JavaScript
SDK or custom client framework is not required.

## 7. Mock publication and event isolation

Planned amendment: [ADR-0057](0057-typed-mock-event-sources-and-consumers.md)
defines a future clean cutover for Mock API names, active timed sources and
registered transactional consumers. It explicitly extends Mutation-only
publication and exact-topic source authority. This section remains the current
implemented baseline until that planned contract is implemented; its Rust,
transport and physical-ownership boundaries remain preserved.

Provide generated, coordinate-specific typed publication and source-binding
contracts. The author selects a bounded topic and publishes a payload of the
subscription field's output type. Reject type/topic/permission errors in both
static contracts and runtime host checks. Do not ask authors to serialize
payloads manually or infer completion from their bodies.

Subscription setup declares its topic binding and optional delivery policy in
a short, bounded Luau invocation. The host owns the resulting subscription.
There is no suspended VM, retained Lua closure/coroutine, polling loop, library
loader, filesystem access, or outbound network access. Ordinary nested fields
continue to use the existing resolver form. New subscription descriptors remain
closed and typed; they do not restore completion `state` or unfinished scans.
Generated intentional `NOT_IMPLEMENTED` entrypoints remain valid contracts.

Only authorized Mutation execution may publish. Pending publications belong to
the request candidate, together with all roots and hooks. Validate and bound
payloads, references, event counts, bytes, and staging work before the commit
fence. Invalid publication or exhaustion remains a sticky request failure,
including when caught by `pcall`. Hooks and Query/event execution do not acquire
publication privileges through aliases or forged tables.

Publish to subscribers only after the entire request commits successfully.
Later-root, hook, output-validation/encoding, deadline, and pre-commit
cancellation failures discard both data changes and pending events. Preserve
commit and within-request publication order. No subscriber may observe candidate
state. A committed request is not rolled back because its HTTP response or a
subscriber's delivery later fails.

Post-commit fan-out must not run user code, await a client, or make the atomic
store exchange depend on a slow subscriber. Use bounded host-owned staging and
dispatch. If a subscriber cannot retain events without loss under the default
policy, terminate it rather than silently drop events or block future Mutation
commits indefinitely. Other subscribers and the committed data remain valid.

Registration has an explicit linearization point: only publications after the
binding is registered are eligible. No automatic replay or gap-free initial
Query-to-subscribe transition is implied. Applications requiring continuity
provide durable event/cursor semantics themselves.

Each event uses a fresh read-only execution context, state initial values,
enabled hook lifecycle, request budget, and current server clock-policy view.
All fields within that event share its consistent store/clock view. Do not
inherit the publishing Mutation's headers, state patch, or local time override,
or retain event-local state into the next event. World clock changes affect
later event contexts under ADR-0046, not monotonic queue/send deadlines.

Retain Mock's existing business-failure classification and all-or-nothing result
policy within each event; do not introduce ordinary Rust partial-data behavior
as a side effect. An ordinary business-error event is not by itself a source
failure. Internal VM/store failures and hard-limit failures terminate the
affected subscription while leaving the worker usable for later requests.

Capture event payloads immutably and preserve validated Record provenance across
the host delivery boundary. The existing private Record versus arbitrary DTO
rules still apply; serialization/reconstruction must not bless a forged record.
The published root remains the immutable captured value, including its private
stored slots. Declared relations from a genuine captured Record resolve
automatically against the fresh event's current committed store snapshot. Derive
relation keys only from sealed captured Record fields and the declared model
relation contract; never rebind the root to a current row, compare its digest to
a later row, or grant a DTO/embedded value relation authority from an ID. Root
update, deletion, or identity recreation does not replace or invalidate that
captured root. Related lookups share the event's one snapshot and retain the
existing missing-target, nullability, list, privacy, and read-only rules.
Resolve and project through each subscriber's context and selection, not a
publisher-authorized precomputed response. Separate servers share no topics or
state. Restart closes subscriptions and restores seeds without replay.

## 8. Transport profiles and lifecycle

Implement the modern WebSocket `graphql-transport-ws` protocol, GraphQL-over-SSE,
and Apollo's HTTP multipart subscription protocol, not three bespoke envelopes.
The result and policy owner is common; bridges implement negotiation, framing,
upgrade/body integration, and bounded writer/timer adaptation.

Before implementation, pin protocol revisions and state the exact supported
profile, including SSE connection modes, method/media negotiation, WebSocket
initialization/credentials and operation-ID rules, heartbeats, terminal errors,
and cancellation. Do not claim all SSE modes merely because one works. The old
`subscriptions-transport-ws` protocol, multipart upload, and `@defer`/`@stream`
incremental delivery are not included by this decision.

Bound request heads retained by a subscription, initialization payloads,
protocol messages, pending setup operations, active operations per connection,
and shared writer/control buffers. Preserve browser origin/CSRF policy and
application authentication; supporting WebSocket must not create an unguarded
alternate entry point. Bound handshake and cleanup waiting as well as event
processing. Fairly drive multiplexed operations without unbounded task fan-out.

The application can serve these transports through its existing HTTP listener.
Document proxy buffering, heartbeat/idle-timeout interaction, and application
HTTP-version choices. Writing to an HTTP body or socket does not establish
client receipt or application progress. Never discard a partially sent message
to make queue space or reinterpret a protocol fault as successful completion.

## 9. Implementation preparation and migration

This section is the SU-WP0 contract freeze. It preserves every accepted default
and hard maximum in Section 5 and closes the remaining API, ownership, protocol,
bound, and migration decisions before executable code changes. These are safety
baselines, not measured capacity or workload tuning. Any implementation that
needs a different contract must amend and re-review this section first.

### 9.1 Neutral Rust source and event contract

Generated ABI 8 uses the existing schema-wide application error `E` and mapper.
For each subscription-root field, its generated resolver group contains three
coordinate-specific pieces:

1. an associated owned event type, `Send + Sync + 'static`;
2. `subscribe_<field>`, called exactly once with coerced arguments by reference
   and a freshly prepared setup context; and
3. `resolve_<field>_event`, called sequentially for every successful source item
   with the same coerced arguments, a borrowed owned event, and a separately
   prepared event context.

If the schema uses the subscription root object as an ordinary nested output,
the same generated group also contains the ordinary `<field>` method with a
typed parent. It is not a source initializer. One application value implements
the one generated group and assembly still requires the complete group in schema
order. No second resolver registry, generated DTO layer, macro, or stringly
coordinate is introduced.

The public neutral wrapper has this semantic contract; exact module placement
may follow existing generated-layout responsibilities without changing it:

```rust,ignore
pub struct SubscriptionSource<T: Send + Sync + 'static, E: Send + 'static> {
    stream: std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<T, E>> + Send + 'static>,
    >,
    overflow: SubscriptionOverflow,
}

impl<T: Send + Sync + 'static, E: Send + 'static> SubscriptionSource<T, E> {
    pub fn new<S>(stream: S) -> Self
    where
        S: futures_core::Stream<Item = Result<T, E>> + Send + 'static,
    {
        Self {
            stream: Box::pin(stream),
            overflow: SubscriptionOverflow::Backpressure,
        }
    }

    pub fn with_overflow(mut self, overflow: SubscriptionOverflow) -> Self {
        self.overflow = overflow;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionOverflow {
    Backpressure,
    DropOldest,
}
```

`Backpressure` is the constructor default. `DropOldest` is selected only by
server code during source initialization; a GraphQL argument, request extension,
or protocol message cannot select it. The wrapper is not `Clone`, exposes no
polling escape hatch, and owns a pinned stream. The source and all its items must
be independent of the borrowed setup context when `subscribe_<field>` returns.
Compilation must reject a source that borrows the resolver call, its arguments,
the prepared context, or request metadata. Application-internal buffering before
an item reaches this stream is outside Chigiri's byte guarantee and must be
documented by the application.

`subscribe_<field>` returns
`Result<SubscriptionSource<Self::<Field>Event, E>, E>`. The outer error is a
setup/source-initialization error. An `Err(E)` stream item is a source-terminal
error. `resolve_<field>_event` returns the field's generated output type or `E`.
Each of those three application errors crosses the existing mapper exactly once.
Validation, coercion, completion, limit, cancellation, and internal errors never
cross that mapper. A source `None` is successful completion; an `Err` is not.

The source is demand-driven and sequential. Chigiri polls no more than one next
item per subscription while no event resolver is active, never executes two
events from one subscription concurrently, and drops the source on unsubscribe,
disconnect, terminal failure, rejected setup, or shutdown. The common owner may
pause polling before acquiring another item. It never creates one task per event.

This complete example is normative for method roles and ownership, while names
outside the shown generated API may move only with an ADR amendment:

```graphql
schema { query: Query subscription: Subscription }
type Query { subscriptionView: Subscription! }
type Subscription { presence(teamId: ID!): Presence! }
type Presence { userId: ID!, online: Boolean! }
```

```rust,ignore
use chigiri::{FieldContext, Id, SubscriptionOverflow, SubscriptionSource};
use futures_util::stream;

#[derive(Clone)]
struct AppContext {
    viewer: Id,
}

#[derive(Debug)]
struct AppError;

impl std::fmt::Display for AppError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("presence service failed")
    }
}

impl std::error::Error for AppError {}

struct Types;
struct Presence {
    user_id: Id,
    online: bool,
}
struct PresenceChanged {
    user_id: Id,
    online: bool,
}
struct SubscriptionView;

impl generated::SchemaTypes for Types {
    type Presence = Presence;
    // Generated only because Subscription is also an ordinary nested output.
    type Subscription = SubscriptionView;
}

struct SubscriptionFields;

struct QueryFields;

impl generated::QueryResolver<AppContext, Types, AppError> for QueryFields {
    async fn subscription_view<'a>(
        &'a self,
        _ctx: FieldContext<'a, AppContext>,
    ) -> Result<SubscriptionView, AppError> {
        Ok(SubscriptionView)
    }
}

impl generated::SubscriptionResolver<AppContext, Types, AppError>
    for SubscriptionFields
{
    type PresenceEvent = PresenceChanged;

    async fn subscribe_presence<'a>(
        &'a self,
        ctx: FieldContext<'a, AppContext>,
        args: &'a generated::SubscriptionPresenceArgs,
    ) -> Result<SubscriptionSource<Self::PresenceEvent, AppError>, AppError> {
        authorize_team(ctx.context(), &args.team_id)?;
        let event = PresenceChanged {
            user_id: ctx.context().viewer.clone(),
            online: true,
        };
        Ok(SubscriptionSource::new(stream::iter([Ok(event)]))
            .with_overflow(SubscriptionOverflow::DropOldest))
    }

    async fn resolve_presence_event<'a>(
        &'a self,
        ctx: FieldContext<'a, AppContext>,
        args: &'a generated::SubscriptionPresenceArgs,
        event: &'a Self::PresenceEvent,
    ) -> Result<Presence, AppError> {
        // This check uses the fresh event context and current authorization data.
        authorize_team(ctx.context(), &args.team_id)?;
        Ok(Presence {
            user_id: event.user_id.clone(),
            online: event.online,
        })
    }

    async fn presence<'a>(
        &'a self,
        ctx: FieldContext<'a, AppContext>,
        _parent: &'a SubscriptionView,
        args: generated::SubscriptionPresenceArgs,
    ) -> Result<Presence, AppError> {
        authorize_team(ctx.context(), &args.team_id)?;
        Ok(Presence {
            user_id: ctx.context().viewer.clone(),
            online: false,
        })
    }
}

struct PresenceFields;

impl generated::PresenceResolver<AppContext, Types, AppError> for PresenceFields {
    async fn user_id<'a>(
        &'a self,
        _ctx: FieldContext<'a, AppContext>,
        parent: &'a Presence,
    ) -> Result<Id, AppError> {
        Ok(parent.user_id.clone())
    }

    async fn online<'a>(
        &'a self,
        _ctx: FieldContext<'a, AppContext>,
        parent: &'a Presence,
    ) -> Result<bool, AppError> {
        Ok(parent.online)
    }
}

fn authorize_team(_ctx: &AppContext, _team_id: &Id) -> Result<(), AppError> {
    Ok(())
}

fn map_app_error(error: AppError) -> chigiri::FieldError {
    chigiri::FieldError::internal(error)
}

fn assemble() {
    let resolvers = generated::Resolvers::builder(map_app_error)
        .with_query(QueryFields)
        .with_subscription(SubscriptionFields)
        .with_presence(PresenceFields)
        .build();
    drop(resolvers);
}
```

The event path runs the normal field lifecycle, authorization plugins,
completion, aliases, abstract-output dispatch, error masking, null propagation,
and response-extension merge. The pre-resolved event is an input to the generated
event method, not a completed GraphQL result. Ordinary nested use calls
`presence`, not `subscribe_presence` or `resolve_presence_event`.
Its ordinary parameter order remains context, borrowed typed parent, then owned
arguments, as for other nested fields under ADR-0053.

The `Sync` event bound is required because the generated event method borrows
the owned event across its `Future + Send` result. Compile fixtures must accept a
normal `Send + Sync` event and reject a `Send`-only event, in addition to the
borrowed-source and incomplete-group failures.

`futures-core` 0.3.34 is the only new direct public stream-trait dependency; it
is already present transitively in the starting lockfile. It is MIT OR
Apache-2.0, declares Rust 1.36, and keeps `chigiri_core` standard-library-only.
The facade re-exports the wrapper; it exposes no Tokio, Hyper, Axum, or
WebSocket type. The starting workspace requirement is Axum 0.8.4-compatible and
the lockfile resolves Axum 0.8.9. Bridge work qualifies that locked Axum 0.8.9
release with its `ws` feature and its corresponding `tokio-tungstenite` 0.29.0
release. The Hyper bridge's direct requirement uses the same 0.29.0
implementation with default, connect, and TLS features off and only the server
handshake enabled. Axum's published `ws` feature enables the upstream default
`connect` and `handshake` features; Cargo may therefore unify `connect` when one
application builds both bridges. No Chigiri API invokes the client connector,
and no TLS feature is enabled. Both crates are MIT and declare Rust 1.80 and
Rust 1.63 respectively, below the bridge MSRV. SU-WP3 must pin the direct stream
and Hyper WebSocket requirements to these reviewed releases, update the lockfile
without unrelated upgrades, and recheck exact build graph, licenses, duplicate
versions, and unified features. This review does not authorize opportunistic
upgrades or vendoring.

The raw socket adapters additionally name the already locked `futures-sink`
0.3.34 trait directly, with its default features disabled. It has the same
MIT OR Apache-2.0 license and Rust 1.36 requirement as `futures-core`. Hyper
names both trait crates; Axum already names `futures-core` and adds only the
Sink trait edge. Axum's application-owned raw upgrade also directly names the
existing `hyper`, `hyper-util`, and `tokio-tungstenite` dependencies. These
direct edges do not add a package/version, TLS feature, client invocation, or
dependency to core. The existing upstream Axum default/connect feature union
remains; direct edges cannot disable features already enabled elsewhere.

Both adapters share one fixed-storage passive accepted-prefix observer in
`chigiri_http`. It counts only application-text payload bytes actually accepted
by the underlying writer, excluding RFC headers and control payloads. It does
not generate RFC frames, own a socket, or replace the reviewed ecosystem
implementation. The adapter retains the original immutable frame owner through
upstream flush and every physical reference. The dependency supplements are
recorded in `lab01-uid.5.3`; observer, post-edge build, and actual socket evidence
remain distinct from that prerequisite qualification.

### 9.2 Fresh preparation and lifecycle ownership

The endpoint owns one bounded `SubscriberRequest` after operation decode and
validation. It contains the method, path and raw query, request headers,
application metadata, peer address, trace identity, decoded GraphQL request,
selected operation identity, coerced variables/arguments, transport profile,
and the already authorized optional WebSocket `connection_init` payload. It
never stores the raw request body, publisher credentials, a prepared `C` or
response state `S`, a field future, or a VM. Debug and diagnostic output remains
redacted.

The byte charge is the checked sum of every owned string and byte value in that
record, including names, header values, query text, variables/extensions, the
initialization payload, and collection element overhead defined as 16 bytes per
entry. Fixed Rust object headers, Arc control blocks, allocator slack, parsed
schema/document cache entries, and application-owned values are outside this
specific byte limit. The endpoint rejects retention before source initialization
if the checked sum exceeds the configured metadata limit.

WebSocket initialization uses one distinct neutral SPI configured through the
same Components/sugar and typed-`inject` leaf. Its default accepts after the
ordinary pre-upgrade admissions; replacing the leaf replaces it wholly.

```rust,ignore
pub trait ConnectionInitAuthorizer: Send + Sync + 'static {
    fn authorize<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>>;
}

pub enum ConnectionInitDecision {
    Accept,
    Reject,
}
```

`ConnectionInitContext` is a borrowed, read-only view of the admitted HTTP
headers, application metadata, peer address, and the bounded optional JSON
object. It contains no request body, GraphQL operation, mutable response state,
or raw logging representation. `Reject` closes with 4403 and a fixed safe
reason. `ConnectionInitError` is an opaque application/internal error and closes
with 1011; neither path exposes its source. Parsing plus authorization must
finish within the ten-second `connection_init` deadline measured from upgrade;
expiry closes 4408. Only `Accept` permits `connection_ack`, operation admission,
or retention of the initialization object. The callback creates no `C`, `S`, or
response completion and runs exactly once for the one allowed init message.

`RequestContext` gains closed phase views for every operation kind:

```rust,ignore
pub enum WebSocketOneShot {
    Query,
    Mutation,
}

pub enum RequestPhase {
    Finite,
    WebSocketOperation { operation: WebSocketOneShot },
    SubscriptionSetup { transport: SubscriptionTransport },
    SubscriptionEvent { transport: SubscriptionTransport, sequence: u64 },
}
```

The optional WebSocket initialization object is visible read-only through the
subscription phase view. It never replaces HTTP headers or application metadata.
The same configured `ContextFactory<C, S>` is called for setup and again for
every event. Setup uses a new request identity and the setup deadline. Every
event uses another new request identity, cache/budget state, and the combined
event preparation/execution deadline. The original bounded credentials are
present each time so the factory and root/argument policies can consult current
revocation and authorization services. This is revalidation with the original
credential material, not credential refresh or authentication of bytes already
sent.

The setup `C` may be borrowed by `subscribe_<field>` only until its future
returns. It and its `S` are then dropped. Event `C` is held only through that
event's execution; event `S` and an event-attached response completion are
dropped without running a response hook because response headers already exist.
Factories can branch on `RequestPhase` and must not attach finite-response-only
resources to event phases. Deliberately shared services must therefore be
application-owned handles placed into each new `C`, not retained request state.

A WebSocket Query or Mutation calls the same `ContextFactory` once with
`WebSocketOperation`, sees the authorized retained init object, and keeps its
normal one-shot execution semantics. A WebSocket subscription uses
`SubscriptionSetup` and then `SubscriptionEvent`. No WebSocket operation runs
the HTTP response hook. Its response state and any attached
`ResponseCompletion` are dropped uncalled when that operation's context is
dropped; bridge rustdoc must state that callers use router middleware and the
protocol lifecycle instead. Query/Mutation produce one bounded `next` followed
by `complete`; subscription setup/event use their protocol terminal rules.
Credentials never flow from one connection or operation generation to another.

WebSocket decoded-request and context-factory views use `RequestMethod::Post` and
`ResponseMediaType::GraphQlResponseJson` as their logical execution policy;
this permits Mutation without applying HTTP GET's query-only restriction.
The retained `TransportRequest::method()` remains the actual upgrade `GET`.
`RequestContext::phase()` distinguishes the one-shot, setup, and event phases,
and its raw body remains empty. These logical values do not perform another
HTTP request or media negotiation, rerun transport admission, authorize an
operation, or enable an HTTP response hook. No new decoded-hook SPI or phase
is introduced for this convention.
The existing operation-source resolver still receives only the decoded request
envelope; it gains no transport-method or initialization-payload input.

For SSE and multipart, successful source setup occurs before the HTTP response
is committed. The existing response hook runs exactly once with the setup `S`
and a `GraphQlResponse` whose `data` is `None` and whose errors/extensions are
empty, then its headers are applied to the initial streaming response. Hook
failure or timeout drops the source and rejects before streaming. Its response
completion, if any, runs once when initial response preparation succeeds or
fails; it does not wait for the subscription lifetime.

The two HTTP streaming profiles share this complete pre-source outcome table.
"Finite" means the existing negotiated GraphQL-over-HTTP error response rather
than a partially established streaming body.

| Last completed stage | HTTP/profile result | Source ownership | Response hook and completion |
| --- | --- | --- | --- |
| Transport admission, method/media/body decoding, operation-source resolution, or decoded hook rejects/fails | Existing finite rejection, masked internal response, or timeout; no SSE/multipart body | No source exists | No setup context, hook, or completion exists |
| Selected HTTP Subscription cannot acquire pending capacity or the pre-Phase-A initial-response byte bundle, or joined preparation cannot transfer to active capacity | Finite 503 with safe `SUBSCRIPTION_LIMIT_EXCEEDED`; no Retry-After promise or streaming body | No source exists; no task is spawned on pending or byte-bundle rejection, and preparation has fully joined before active transfer | No setup context, hook, or completion exists |
| A pre-source quota/ticket transition has an invariant or generation failure | Existing finite masked 500 through the internal observer path; no resource details or streaming body | No source exists; any acquired pending/ticket ownership is released exactly once | No setup context, hook, or completion exists |
| Raw owned-ingress retention fails its lower-bound guard before validation starts | Finite 413 with safe `BAD_USER_INPUT`; no streaming body | No task or source exists; the acquired pending/ticket owner is released exactly once | No setup context, hook, or completion exists |
| GraphQL parse, validation, selection, or coercion returns a request error | 200 selected stream profile: SSE writes one `next` then `complete`; multipart writes one projected top-level error part then closes | No source exists | Context construction has not run, so the hook and completion run zero times |
| Setup `ContextFactory` returns `PrepareError::Reject`, internal error, or timeout | Existing finite rejection status, masked internal response, or timeout | No source exists | No prepared state exists; the hook and completion run zero times |
| `subscribe_<field>` returns `Err(E)` | 200 selected stream profile with the mapped error: SSE `next` plus `complete`; multipart projected top-level error plus close | The initializer returned no source | The hook runs once with the mapped `GraphQlResponse`; additions apply to initial headers. Its completion reports `Encoded`, `EncodingLimitExceeded`, `Failed`, or `TimedOut` for this bounded response preparation |
| Source succeeds and the hook succeeds | 200 selected streaming response and a live source | The delivery owner takes the source only after headers/body ownership is committed | The hook runs once with the empty initial response; additions apply. Completion reports `Encoded` after bounded initial response preparation, not after stream lifetime |
| Source succeeds but the hook fails or times out | Existing finite masked internal response or timeout; no stream is committed | The newly created source is cancelled and dropped | The attempted hook is not retried. Completion runs once with `Failed` or `TimedOut` |
| Any event/source/limit/transport failure after response commit | The already selected protocol's safe terminal behavior; status and headers do not change | Normal terminal cleanup or quarantine applies | The hook and initial completion never rerun |

A parse-only routing hint never authorizes source setup. A positive Query or
Mutation retains finite negotiation, while a positive Subscription selects its
supported profile. If parsing or operation selection is indeterminate, keep the
ordinary finite error path when finite media is acceptable. Otherwise a supported
stream-only POST proceeds through the same bounded request-error preparation;
complete validation still precedes any context or source work. Unsupported
profiles, GET restrictions, earlier finite rejection, pending capacity, and raw
retention precedence remain unchanged. This does not infer an operation from
keywords or reinterpret malformed syntax as permission to execute a source.

All internal errors use the existing masking and observer path. A multipart
request error/source error is projected only after the hook view is formed, so
the hook sees the ordinary safe `GraphQlResponse` while the wire still omits
top-level `locations` and `path`. Hyper and Axum use the same table and fixtures.

Pre-worker finite rejections use the provisional finite media selected during
ingress, or `application/graphql-response+json` when the request admitted no
finite representation. They do not mislabel an error body as SSE/multipart.
Capacity finishes the request observation once as `Rejected` with
`SUBSCRIPTION_LIMIT_EXCEEDED`; invariant failure finishes as `Failed` with
`INTERNAL_SERVER_ERROR`; raw retention rejection finishes as `Rejected` with
`BAD_USER_INPUT`. Their standalone in-flight guard is released after bounded
initial response preparation. An actual future or unconsumed handoff Drop,
rather than a completed rejection, records `Cancelled`. The final exact
prepared-metadata guard still belongs to validated subscription preparation;
its safe request error follows the selected-profile row, not the earlier raw
ingress 413 branch. No pre-source failure changes or evicts a live subscription.

A WebSocket HTTP 101 necessarily precedes `connection_init` and operation setup.
Therefore a per-GraphQL-response hook cannot modify it and is not run for
WebSocket subscribe/event messages. Application router middleware owns extra
101 headers. Chigiri still runs transport admission, CSRF/origin checks, and the
bounded handshake before upgrade, and runs decoded hooks/context preparation for
each `subscribe`. No later event or operation can mutate the 101 headers. This
transport distinction must be stated in both bridge rustdoc and examples.

### 9.3 Complete finite bounds and accounting

The first table in Section 5 is unchanged. The additional accepted defaults are
completed as follows. Equality with a hard maximum is valid.

| Guardrail and scope | Default | Hard maximum |
| --- | ---: | ---: |
| Chigiri-owned encoded bytes per Endpoint, shared by clones | 64 MiB | 256 MiB |
| Pending operation setups per Endpoint | 16 | 128 |
| Retained subscriber request metadata per subscription | 16 KiB | 64 KiB |
| Pending publications per complete Mock Mutation request | 32 | 1,024 |
| Pending publication payload bytes per complete Mock Mutation request | 1 MiB | 16 MiB |
| Heartbeat interval for the selected profile | 15 seconds | 60 seconds |
| Open Chigiri WebSocket protocol connections per Endpoint | 64 | 512 |
| Pending setup operations per WebSocket connection | 8 | 32 |

Every value is positive. Heartbeats additionally have a one-second inclusive
minimum. WebSocket connection/setup settings live in one protocol-settings
value; publication settings live in one Mock-publication value. Replacing a
group replaces it wholly. Sugar and `inject` write the same slots.

The following private constants are fixed in ABI/format 8/9 and are not public
setters. Changing one requires review because it affects memory or protocol
behavior:

| Internal bound | Value and scope |
| --- | --- |
| Inbound WebSocket JSON message | 1 MiB per complete text message |
| WebSocket `connection_init` payload | 16 KiB encoded JSON, included in retained metadata |
| WebSocket operation ID | 256 UTF-8 bytes; nonempty and unique among active IDs |
| WebSocket ping/pong payload | 16 KiB encoded JSON |
| Outbound non-event control message | 64 KiB |
| Initial HTTP response, including callback result | 64 KiB for body bytes plus UTF-8 header-name/value bytes plus 16 bookkeeping bytes per header entry |
| Queued outbound control | 32 messages and 256 KiB per WebSocket connection |
| Pending HTTP-stream heartbeat | one coalesced marker per SSE or multipart stream; encoded frame at most 64 KiB |
| HTTP upgrade and protocol negotiation | 10 seconds |
| WebSocket `connection_init` receipt | 10 seconds after upgrade |
| Owned cancellation/cleanup wait | 5 seconds per connection or subscription owner |
| Mock topic | 1 through 256 UTF-8 bytes, excluding ASCII control characters |
| Bindings per Mock subscription | exactly one topic |
| Drop counter and event sequence | at most 9,007,199,254,740,991 (`2^53 - 1`) |
| Encoded delivery-counter value slot | 16 ASCII bytes per event, including insignificant trailing JSON spaces |
| Mock subscription log queue | 64 records shared by the four subscription record kinds; each JSONL record is at most 16 KiB including its newline |

An oversized inbound WebSocket message closes the connection with 4400. An
oversized initialization payload closes with 4400; an initialization timeout
uses 4408. Oversized IDs reject that operation with an `error` when the message
can be identified safely, otherwise 4400.

Logical terminal ownership and physical cooperative cleanup are distinct. At a
terminal decision Chigiri revokes the generation's emission capability and wire
ID immediately, signals cancellation, and waits actively for at most five
seconds. Expiry moves the task/future handle and all of its permits and byte
tokens into a generation-keyed endpoint quarantine; it does not release or
detach them. An HTTP owner retains its active-subscription permit. A WebSocket
connection remains charged against the endpoint connection limit, and its late
operations retain their active-operation permits, until all quarantined work
actually returns and drops its tokens. Those original limits therefore bound
quarantine cardinality without another orphan allocation budget.

Quarantined work has no writer, topic-index, wire-ID, or new-generation handle.
Late completion can only idempotently release tokens held by its own generation;
it cannot emit, delete a reused ID, change a new owner, or release twice. The
five-second value bounds active waiting and transport shutdown, not arbitrary
application code or physical future destruction. Rust cannot preempt a
synchronously blocking `poll`; such code can retain capacity until it returns
or the process stops, but cannot create uncharged owners through repeated ID
reuse. This residual application responsibility is observable with safe counts
and never raw credentials or values.

The official Tokio bridges use a concrete, move-only worker completion owner;
dropping a raw Tokio task handle is not cleanup because it detaches execution.
Runtime-specific ownership is behind an internal optional facade feature enabled
by the official bridges. The default neutral facade and `chigiri_core` remain
Tokio-free, and neutral public contracts expose no Tokio handle or generic
application-future cleanup constructor. Existing finite service/handler
construction remains unchanged; streaming integration explicitly owns its
runtime guard and drain lifecycle alongside the application's runtime.

There is one generation-keyed completion-slot namespace, shared by local
quarantine and final process supervision. Its checked capacity is
`P + C * A + S + C`, where `P` is endpoint pending setups, `C` is open WebSocket
connections, `A` is active operations per connection, and `S` is endpoint active
subscriptions. The categories cover pending workers, active WebSocket
Query/Mutation/Subscription workers, HTTP subscription workers, and connection
reader/writer/heartbeat aggregates. A WebSocket subscription's two quota charges
belong to one operation record. Admission reserves the exact cleanup ticket
before work can escape; sequential preparation and source workers use the same
ticket. Neither worker phase, ID reuse, nor connection teardown adds an orphan
pool, unbounded channel, or unnamed worker allocation budget.

A stable endpoint completion table is registered fallibly before streaming
workers start. One process-scope reaper owns an OS thread and its thread join
handle; it creates no Tokio runtime and shares no quota between endpoints. It
polls only concrete official completion records, never arbitrary source or
application futures. Local cleanup and the reaper exchange exclusive polling
ownership of the same reserved slot. A record keeps its concrete task join
handle, ticket, physical permits, and byte owners inseparable. Last-local-owner
or unexpected runtime-guard Drop aborts and transfers polling ownership without
allocation or application callbacks; the registered process owner retains the
record until actual join completion even when every Endpoint and local cleanup
driver has gone away. Source tasks capture no EndpointState, completion table,
reaper, writer, topic index, logical ID, or newer generation.

Polling and destruction run outside completion-table locks. A ready join is the
physical-completion witness, not an abort request or task-side flag. Returned
application values and panic payloads are disposed before the physical charge.
Destructor/poll invariant failures poison the slot and retain the complete
charge fail-closed; they cannot be reported as a successful drain. Reaper wake
notifications never recursively poll a task or form a task-to-slot-to-handle
strong cycle. An abandoned endpoint registration is removed only after all of
its slots have actually drained.

Endpoint shutdown closes its own admission, revokes and collects its operation
and connection generations, drains only its table, and unregisters it. It does
not stop another endpoint or application runtime. Servers may register again
while process supervision remains running. Final process supervision shutdown
is separate and one-shot: reject new registrations, require endpoint guards to
close, drain all records, stop and join the reaper thread, and only then destroy
the application-owned Tokio runtimes. Registration after final shutdown fails
explicitly. Unexpected Drop is never reported as successful shutdown, but still
leaves concrete completion ownership available for eventual collection.

Preparation uses two sequential tasks, never an activation-grant-waiting task
or per-event task fan-out. An already charged pending worker validates/coerces
bounded owned ingress, including application scalar callbacks. Its original
setup/HTTP-overall timers stay independently pollable outside blocked
application work. Only a fully joined validation result can transfer atomically
to active admission; failure has no live uncharged task. One active source
worker then owns setup and the entire sequential event loop. Setup and event
deadlines must likewise remain enforceable while application code blocks
synchronously; idle source waiting does not start an event-execution timer.
No handoff, spawn, active transfer, or phase notification resets a deadline.
HTTP overall timing starts before admission/decode, and WebSocket handshake and
initialization deadlines remain distinct.

Before Phase A starts, HTTP preparation reserves one maximum 64-KiB initial
response buffer and the exact charges for two separate fixed callback inputs:
masked finite 500/`Failed` and finite 504/`TimedOut`, both in the original
provisional finite media. All reservations precede candidate allocation. Failure
to reserve any part returns finite 503 `SUBSCRIPTION_LIMIT_EXCEEDED`, records
one `Rejected` observation, and releases the exact pending permit and ticket.
It adds no Retry-After, evicts no operation, and starts no validation task,
context, source, hook, or completion. The fixed candidates and maximum result
buffer share the endpoint byte owner, not a new allocation budget.

An armed, move-only HTTP initial-completion token belongs to the same already
reserved concrete source task as its source/session, never to a queued setup
result or the reaper. Capacity-one command/result ports connect that task to
one outer initial-response owner. The task owns callback values but no physical
resource token. The outer owner retains the exact generation, original deadline
table, observation, permits, fallback charges, and maximum result reservation.
The task-local ordered owner exists before initialization begins. A successful
setup context moves its unique initial completion into that owner before source
polling or a hook await. A newly established session moves into the same owner
before fallible deadline clears and remains there through hook preparation.
Thus executor destruction before a setup result exists still destroys the
source first and attempts the precharged Failed completion once. Initializer
failure honors any already selected terminal command before acknowledging its
result; it never silently drops an armed token during an early return.
Setup ends only after setup/context/source/hook preparation; Overall remains
the original ingress timer and is never recreated at a handoff.

After bounded initial encoding, the outer owner polls Overall first. A ready
deadline wins an exact result race. Otherwise it linearizes `Committing` once
and sends the single prepared response and outcome to the source task without
cloning. The token disarms once before invoking the callback. `Encoded` describes
the bounded preparation/transport result immediately before callback execution,
not synchronous application callback CPU time. After `Committing`, Overall is
physically retained through callback result ownership, deadline-table clear,
and worker transfer but cannot retroactively replace that outcome with timeout.
Successful initial commit alone transfers the source into event delivery.
The committed HTTP initial wire owner and its exact-generation source/cleanup
share the same existing active-subscription permit. Even an empty initial body
retains that physical lease; source JoinReady alone cannot admit a replacement
while the last response reference remains. Lease issuance precedes fallible
commit and byte transfer. Failed publication retains no writer lease in a
finite or closed failure result. The official supervisor must extend that same
lease to each body and event/control frame through its last physical Drop;
this is not another permit, task, or quota.

At HTTP source termination, the common owner consumes the exact outstanding
pre-demand reservation only after every earlier queued or physical event has
finished. A foreign reservation or an unfinished event is an invariant failure,
never permission to replace data with successful completion. A source error is
encoded within that reservation and the 64-KiB control ceiling; insufficient
capacity or encoding failure emits no partial or uncharged fallback. SSE uses
one error `next` with the current delivery counter and then a separate complete
entry; multipart uses projected top-level transport errors and then its final
boundary. The completion entry is exactly reserved, reusing a sufficiently
large pre-demand reservation for natural completion or acquiring its known
static length before encoding. No subsequent control is staged while the prior
physical control owner remains alive. Cancellation emits nothing. These controls
do not enter the evictable event queue or acquire an event send-stall deadline.

All fallible deadline/source commit checks precede the maximum-to-exact byte
transfer. A failed validation or shrink preserves the original reservation;
masked finite failure uses it without a new reservation or completion attempt.
If exact shrink is unavailable but the intact, uncorrupted maximum token still
covers the bounded failure response, that token stays inseparable from the wire
owner through its last physical Drop. A missing or corrupted physical owner
instead produces a closed non-emitting failure with one `Failed` observation;
it must not emit an uncharged fallback or transfer a source into delivery.

The maximum reservation covers callback execution and the capacity-one result
port. The task checks the callback-returned response's body/header charge with
checked arithmetic before enqueue. The outer owner checks it again on receipt
and only then shrinks to exact wire ownership. Oversize, arithmetic failure,
callback panic, or returned-value disposal panic is contained inside the task
and reported as a small closed failure signal. The finite wire failure uses the
already reserved masked 500 path; completion is never retried or rearmed.

Before `Committing`, timeout revokes emission and commands `TimedOut`; true
caller/result abandonment closes the command port, records `Cancelled` once,
and emits no response. Both routes first destroy the source/session in the same
task and then attempt its one armed completion with the separately pre-owned
504 or 500 callback input. Terminal callback-returned parts are discarded there
before JoinReady. An explicit task-local ordered Drop preserves this order when
runtime abort destroys the future. Source/session destruction, callback unwind,
returned-value destruction, and nested panic-payload disposal are contained
separately; a source destructor panic must not skip the one completion attempt.
No callback runs in an outer Drop, under a lock, or on the process reaper.

The maximum result/wire buffer and fallback charges have distinct move-only
owners. Timeout may return a separately prepared finite wire 504 and finish the
observation `TimedOut` once after cleanup joins or its at-most-five-second
active wait expires;
the task's callback candidates and their exact charge remain with cleanup.
The wire buffer shrinks only against that separate finite response. True owner
abandonment instead transfers the unused maximum buffer into the same retained
generation. If callback or source destruction blocks, the same concrete join,
timers, fallback charge, applicable buffer, and physical permits move to the
existing quarantine. A late task cannot emit, rerun observation, or affect a
new generation. JoinReady and value disposal remain the outer physical-release
witness; the reaper never executes deferred application callbacks.

The same capacity-one result port carries a closed terminal acknowledgment.
When cleanup joins inside the active wait, callback/source disposal failure or
missing acknowledgment selects the already charged masked 500 with one Failed
observation, without retrying the attempted TimedOut completion. Once the wait
has quarantined the task and returned finite 504, a late acknowledgment cannot
replace that response or repeat its observation. True abandonment remains
non-emitting and does not await an acknowledgment.

Closing a sender preserves an already queued result until receiver drain.
Receiver closure extracts queued values outside locks and takes the same
terminal transition; no setup message has an independently armed token that
could finish before source destruction. The official supervisor drives this
initial state independently of body polling. WebSocket and already completed
rejections carry no armed token. Initial completion never waits for stream
termination and never reruns per event or at stream end.

The endpoint byte owner uses checked reservation tokens shared by all endpoint
clones. It charges:

- complete encoded event entries, including GraphQL delivery metadata and the
  profile's immutable event framing;
- the active per-subscription encoding reservation;
- an in-flight event until its final byte is accepted by the bridge body/socket;
  partial writes keep the entire entry charged and non-evictable; and
- queued control/heartbeat/terminal bytes.

Before polling a source for another item, a subscription acquires one reservation
equal to its configured single-event maximum. No more than one such reservation
exists per subscription. Encoding uses a writer capped by that reservation and
never builds an unbounded candidate. On success the token shrinks to actual
encoded bytes and moves, without copying or double charging, through queue and
writer ownership. On error/cancellation it releases. Library frame headers, OS
socket buffers, TLS/proxy buffers, source values, application queues, and domain
heaps are outside this byte number; the application/server must bound those.

The per-subscription byte count includes its queued and in-flight event entries.
The per-subscription event count includes wholly encoded queued entries but not
the one in-flight partial entry. A dequeued entry stops queue-age timing and
starts the send no-progress timer. A timer resets only when at least one byte of
that entry is accepted; heartbeats and other operations do not reset it. Queue
age starts when an encoded entry is admitted. Setup, event preparation/execution,
queue residence, send progress, handshake, and cleanup use separate monotonic
deadlines and never extend one another.

The common pure evaluator receives an immutable snapshot of active counts,
reserved/actual bytes, event count, oldest admission instant, send-progress
instant, candidate size, and selected overflow policy. It returns only
`Admit`, `Pause`, `EvictWhole(count)`, `RejectNew`, or `Terminate(reason)`.
It checks all applicable conditions with OR semantics on admission, reservation,
enqueue, dequeue, byte progress, and timer expiry. The delivery owner performs
the returned action and re-evaluates after every eviction. Protocol bridges do
not contain policy copies.

For `Backpressure`, a full queue stops source demand. Its oldest-entry timer
continues and terminates if capacity does not recover. Shared-budget reservation
failure also pauses source demand; an already held event reservation means that
failure cannot occur after an item is yielded. `DropOldest` may evict only this
subscription's wholly unsent event entries. It may not evict control, an encoder,
or an in-flight/partial entry. An oversized candidate, stalled send, active-count
limit, counter overflow, or invariant failure is never recoverable by eviction.

The pre-demand maximum is endpoint encoding capacity, not a hypothetical queued
event. DropOldest must not evict live queued events for a result that may instead
be pending, complete, or an error. Local count/byte eviction uses the actual
encoded event. Successful encoding immediately shrinks the same token to exact
bytes. If a non-evictable physical event alone prevents candidate admission, keep
that one exact-charged candidate, stop additional demand, and retain otherwise
live queued survivors. Re-evaluate after physical release and evict only as much
as can actually recover admission. Queue expiry and real shared-budget pressure
remain independent conditions; neither candidate waiting nor heartbeats extend
the original send-progress deadline. Backpressure retains its worst-case
pre-demand capacity check because it cannot discard a yielded event.

The first failed shared maximum-event reservation arms a monotonic pressure-wait
deadline derived from that subscription's `max_queue_age`. This is an additional
use of the same validated setting, not a new configurable duration or a change
to when queue residence begins. A successful reservation alone clears the wait;
heartbeats, zero-byte writes, retries, and other operations never restart it.
Expiry terminates the affected subscription with a safe shared-budget pressure
reason, including when it has no queued or in-flight event. Queue-age and send
deadlines remain independently applicable with OR semantics. An idle source
whose demand has not failed a reservation does not arm this pressure timer and
is not a stalled writer. Thus shared pressure cannot create indefinite waiting
without extending or borrowing another subscription's resource budget.

Every WebSocket `subscribe` first atomically acquires both an endpoint pending-
operation permit and a per-connection pending-setup permit. After validation and
operation selection, successful admission atomically transfers the connection
permit to the Section 5 active-operation quota before execution begins. Query
and Mutation retain that permit through their terminal frame and cleanup. A
Subscription additionally acquires an endpoint active-subscription permit
before source initialization and retains both active permits through physical
cleanup. A failed transfer produces an operation `error` with a safe overload
code and releases all pending tokens; it does not evict an existing operation or
close the socket. Duplicate-ID checking and quota transfer occur under the same
connection owner, so every accepted ID is in exactly one pending, active, or
quarantined generation state.

The delivery owner has at most one source driver per active subscription, one
reader/writer/heartbeat owner per WebSocket connection, and no per-event task.
The WebSocket writer visits active operation IDs round-robin and takes at most
one event entry per operation per visit. Terminal close/error controls may
preempt; at most eight ordinary ping/pong controls run consecutively before an
available event turn. SSE and multipart each own one writer because they do not
multiplex. All maps, timer entries, observation records, task handles, and
writer entries are bounded by the connection, pending-operation, active-
operation, active-subscription, and quarantined charged-owner limits above.

Official HTTP streaming services return an explicit application-polled aggregate
driver and an endpoint runtime guard. The driver preallocates at most `S` slots
from the same endpoint graph and fairly polls existing source workers without
creating another task per stream or event. Bodies are capacity-one handoffs, not
worker/timer poll owners. The application may explicitly track a driver task,
must close registrations and drain the driver, then drain its endpoint guard and
the existing process owner before destroying the application runtime. The Axum
streaming service is fallible and must reach the application-owned connection
service without a Router/HandleError conversion into an infallible response.
Existing finite handler/service integration remains unchanged.

The Mock CLI bounds its own accepted TCP connection-task inventory by checked
`HTTP_REQUESTS + S + C`, read from the same endpoint's effective settings (`S`
active subscriptions and `C` protocol connections). This is application-owned
bookkeeping, not another subscription quota or a per-bridge policy default.
Idle/static connections consume this inventory but never GraphQL request
admission; the existing finite request quota remains separate. Before stopping
connections, the application explicitly closes the existing HTTP streaming
registration even while connection-held service clones and bodies remain live.
This wakes cancellation/drain independently of body demand and avoids waiting
for the last clone to close its own active stream. Both official services expose
that idempotent registration-close operation without creating another owner.

Body acceptance timestamps, common policy, and disconnect-versus-demand port
commit share one short timeline authority. Worker wakes, application observers,
source polling, and response payload destruction, including unwind, occur after
its release. A committed source failure selects its masked response once and
waits for any earlier usable physical frame before terminal encoding. A retired
event keeps its original progress deadline until all bytes are accepted; later
physical retention alone is not a stall. A stalled/broken body may close without
a last frame. An aggregate internal fault reaches open bodies as a closed safe
body error, never ordinary successful EOF; explicit shutdown/disconnect remains
non-emitting cancellation.

Finite and selected-profile initial responses retain distinct validated
preparation metadata even when both are fully encoded. Selected SSE/multipart
errors suppress Content-Length and exact body size hints; finite responses keep
their existing exact sizing. Empty prepared bodies retain their original byte
owner until physical body Drop, and nonempty zero-copy bytes retain it through
the last physical reference.

An SSE or multipart stream stores at most one fixed-size pending-heartbeat
marker. Repeated timer expiry coalesces into that marker; no encoded frame is
allocated until the writer is between complete event entries. A partial event
keeps priority and blocks heartbeat encoding without accumulating markers. When
an event is ready at a frame boundary, the event gets the next turn and the
heartbeat is coalesced to a later turn; terminal control may preempt both. The
writer must reserve the exact immutable heartbeat frame against the endpoint
byte owner before encoding. Reservation failure, a frame above 64 KiB, or an
invariant failure terminates that stream and never evicts an event. Heartbeat
progress does not reset an event's no-progress timer.

The delivery-drop counter uses `u64` internally but terminates before increment
would exceed `2^53 - 1`, so its JSON number is exact in JavaScript clients. The
latest cumulative value is inserted during final encoding, after eviction
decisions, rather than captured in an earlier source item. Event-sequence
overflow terminates the subscription before reuse. All arithmetic uses checked
addition/multiplication; overflow is a construction error for configuration and
a safe terminal error for runtime-accounting corruption.

Event encoding writes the reserved counter leaf into one private 16-byte JSON
value slot and records its exact byte position while writing that path; it never
searches application text for a marker. Before the first byte of an event is
emitted, consuming finalization writes either `null` or canonical decimal digits
followed by insignificant ASCII spaces to fill the slot. It uses the current
cumulative count after all preceding eviction decisions, so surviving queued
events cannot carry an earlier count. Finalization and the queued-to-in-flight
handoff form one synchronous ownership transition before asynchronous writer
waiting. Later evictions affect later emissions and never rewrite the already
in-flight event. The entire slot and immutable protocol
framing are included in the capped encoding and all byte charges. Finalization
cannot change length, allocate another candidate, retain or re-encode domain
values, or modify an event after emission begins. The opaque queued encoding is
move-only and exposes no provisional bytes; finalized send bytes have no counter
mutation capability. This private representation preserves ordinary JSON and
the three frozen protocol envelopes without a client extension or SDK.

Mock publication count and bytes are charged while building the complete request
candidate. The charge is the exact length of one private canonical host capture:
one byte per value tag; eight bytes for every signed integer, finite float, or
collection length; exact UTF-8 bytes for strings, IDs, enum values, field names,
topics, and coordinates; 16 bytes per list element, object field, publication,
and batch entry; and the exact 32-byte Record provenance digest plus its model
and public-identity UTF-8 bytes. Null and Boolean payloads use one payload byte.
Objects are captured in generated field order and lists in source order. Shared
or duplicate values are charged each time they are captured; pointer identity
never discounts a publication. All addition is checked.

The capped capture writer owns the typed tags and verified Record provenance and
refuses the next allocation before the per-request 16 MiB maximum is exceeded.
The batch charge includes payload, topic, coordinate, provenance, collection,
and framing bytes; there is no uncharged per-publication metadata allocation.
The topic index is separate fixed host metadata. One server-local shared entry
exists per distinct topic with at least one active binding; it is allocated and
charged before the first binding linearizes and removed only after the last
binding generation leaves. Inside it, each active binding owns an at-most-256-
byte topic reference, coordinate, generation, registration sequence, and fixed
64-byte bookkeeping record charged by its active-subscription permit. Topic
entry and binding cardinality and bytes are therefore bounded by the active-
subscription limit. Reconstruction for a fresh event VM validates the captured
type/provenance again and never converts a private DTO into a Record.

Before the commit fence, the candidate nonblockingly reserves one host queue
slot and `16 + captured_batch_bytes` from the server's 32-batch/16-MiB dispatch
budget. Failure is a sticky pre-commit request failure and commits nothing. The
commit winner moves the buffer and reservation token into the queue without
copying, re-encoding, or double charging. Rollback releases both.

Each shared topic entry also has one allocation-free monotonic
`highest_lost_commit_sequence`, initially zero. Registration and publication
commit take distinct checked values from the same server-local sequence under
the topic-index/store linearization lock. If a committed transfer still cannot
enqueue because of cancellation after the store win, dispatcher failure, or an
internal reservation invariant violation, the short store fence performs a
nonallocating lookup for every topic in the already validated at-most-1,024-
publication batch and raises each existing topic entry to the maximum of its
current marker and this commit sequence. An absent entry has no eligible binding
and needs no marker. This enumerates publications, never bindings, and runs no
VM, fan-out, or wait. Sequence overflow stops later binding and Mutation
admission.

Before any further event delivery, the dispatcher compares a binding's immutable
registration sequence with its shared topic's highest lost sequence. A greater
lost sequence terminates that exact binding generation immediately; this
terminal failure may preempt its older queued events, and no later publication
is delivered to it. Thus a generation registered between two lost commits is
unaffected by the first but is caught when the monotonic marker advances for the
second. A generation registered after the latest loss is unaffected. The marker
is never decreased or cleared while the shared topic entry exists; removal after
the last binding is safe because no eligible owner remains. The server-local
owner prevents cross-server leakage, and the bounded dispatcher visits bindings
incrementally with no publication-by-subscriber task fan-out.

### 9.4 Pinned transport profiles

The protocol contracts are pinned to immutable source revisions:

- `graphql-transport-ws`: `graphql-ws` 6.4.1, commit
  [`7281f1b8f09637ab383c1aab4d14247d683bc15d`](https://github.com/enisdenjo/graphql-ws/blob/7281f1b8f09637ab383c1aab4d14247d683bc15d/PROTOCOL.md).
- GraphQL-over-SSE: `graphql-sse` 2.6.3, commit
  [`4cced455cb7a23beda9e9bf7a403aab270bd4e66`](https://github.com/enisdenjo/graphql-sse/blob/4cced455cb7a23beda9e9bf7a403aab270bd4e66/PROTOCOL.md).
- Apollo multipart subscriptions: the immutable
  [Apollo documentation snapshot](https://www.apollographql.com/docs/deploy-preview/9d04abe4e703ef5acab42d0d/graphos/routing/operations/subscriptions/multipart-protocol).
- Client acceptance and the metadata recipe: `@apollo/client` 4.3.5, commit
  [`f3d28a55e5996429a57bf5367187bf8991464a89`](https://github.com/apollographql/apollo-client/tree/f3d28a55e5996429a57bf5367187bf8991464a89).

Moving links in Section 12 remain discovery aids only. The supported profiles
are deliberately narrower than everything those projects can express.

#### WebSocket

Only an HTTP/1.1 `GET` upgrade negotiating the exact
`Sec-WebSocket-Protocol: graphql-transport-ws` token is supported. Missing or
unsupported subprotocol negotiation is rejected before upgrade. The old
`graphql-ws` token, `subscriptions-transport-ws`, WebSocket-over-HTTP/2 extended
CONNECT, binary GraphQL messages, fragmented messages exceeding the reassembled
message limit, and protocol-level resumption are unsupported.

Both bridges use one borrowed, allocation-free opening-profile check for the
HTTP/1.1 GET, Host, Connection/Upgrade tokens, singleton version 13 and canonical
16-byte nonce, and the exact case-sensitive GraphQL subprotocol. All offered
subprotocol tokens must be unique across the combined request field lines;
case-distinct tokens remain distinct. Invalid input produces a fixed
finite rejection, never 101; unsupported well-formed WebSocket versions advertise
only version 13. This syntax check is not a connection, origin, or CSRF permit.
The reviewed ecosystem WebSocket implementation still computes the accept key
and owns RFC framing. Authority routing, HTTP parser/header limits, and trusted
proxy and allowed-origin policy remain application-owned.

Existing transport admissions and the CSRF guard run before upgrade without
polling a body. Applications retain routing, trusted-proxy, allowed-origin, and
listener policy. The bridge never treats the WebSocket as a same-origin bypass.
After upgrade the client must send one `connection_init` within 10 seconds.
An omitted or JSON `null` payload is accepted as no initialization object; one
JSON object payload is accepted and retained read-only. Other payload shapes are
malformed, and a second init closes 4429. Application rejection of connection
initialization closes 4403. Section 9.2's `ConnectionInitAuthorizer` completes
before `connection_ack`; an opaque internal failure closes 1011 and init
authorization timeout closes 4408. No GraphQL operation context is created
before acknowledgment.
Subscribe before acknowledgement closes 4401. Successful initialization sends
one `connection_ack`; Chigiri sends JSON `ping` every configured heartbeat
interval and answers valid peer `ping` with `pong`. A peer `pong` is observation,
not client receipt or data-send progress.

`subscribe` IDs are bounded strings and unique among active and pending
operations. The wire ID is released as soon as the operation reaches its one
logical terminal state, including receipt of client `complete`, and may then be
reused immediately as the pinned protocol permits. Any still-running bounded
cleanup keeps a private generation/owner token, not the wire ID, so stale work
cannot emit into a later operation that reuses the text. Its resource permits
and byte reservations remain charged until that cleanup releases them, so a new
operation may still be rejected by ordinary capacity admission without being
misclassified as a duplicate ID. Duplicate IDs that are still active or pending
close 4409.
Malformed/unknown messages close 4400. Each accepted subscribe follows Section
9.3's atomic endpoint/connection pending-to-active permit transfer. The
connection-wide active-operation quota covers Query, Mutation, and Subscription
IDs and bounds their maps, writer entries, futures, and terminal cleanup owners.
Query and Mutation operations are allowed on the socket and produce at most one
`next` followed by `complete`; they retain their normal execution semantics and
do not acquire a source. Subscription results use `next`. A setup/validation failure uses
operation `error` and no `complete`; a source/internal terminal failure does the
same when the writer remains usable. Natural source completion and client
`complete` cancel/finish the operation; racing terminal messages are ignored
after the first owner wins. Failure of one operation preserves others unless the
socket or shared writer is unusable.

HTTP headers/cookies and the optional initialization object are the only
credential inputs Chigiri retains. The application interprets them in admissions
and the `ConnectionInitAuthorizer`, then the `ContextFactory` sees the retained
accepted object for each operation. Chigiri neither merges fields nor refreshes
credentials.
Raw credential values, init payloads, queries, and variables never enter public
close reasons, logs, metrics, or Debug output.

#### Server-Sent Events

HTTP stream negotiation applies to an already selected Subscription and does
not turn finite Query or Mutation responses into streams. An explicit matching
`Accept` range is required; an absent header is not an implicit stream opt-in.
For each offered representation, media-range specificity and matching parameter
count take precedence over quality as in RFC 9110 Section 12.5.1. An exact
`q=0` exclusion therefore overrides a positive wildcard. All `Accept` field
lines participate; equal-precedence duplicates use their highest quality.
SSE is the server preference when both stream profiles have equal positive
quality. A wildcard can admit SSE, but multipart additionally requires a
positive matching range containing `subscriptionSpec=1.0`. Malformed ranges
and parameters absent from the fixed offered representation do not match.
One case-insensitive `q` parameter is the weight at any parameter position;
only the other matching parameters contribute to parameter-count precedence.
Negotiation scans borrowed input without input-sized header or range storage.

Only GraphQL-over-SSE distinct-connections mode is supported. A subscription is
an HTTP `POST` with `Content-Type: application/json` and an `Accept` range that
admits `text/event-stream`. Native-EventSource `GET`, single-connection mode,
`PUT` reservation, reservation tokens, operation IDs, and `DELETE` cancellation
are unsupported and receive ordinary pre-stream method/media rejection.

The successful response is 200 with
`Content-Type: text/event-stream; charset=utf-8`, `Cache-Control: no-cache,
no-transform`, and no content length. Results are
`event: next\ndata: <GraphQL response JSON>\n\n`; completion is
`event: complete\ndata: \n\n`. Pre-execution GraphQL request and source errors are
encoded as one `next` followed by `complete`, as required by the pinned profile.
A heartbeat is the SSE comment `:\n\n`. HTTP disconnect cancels the operation;
there is no server claim of client receipt and no replay or `Last-Event-ID`.

#### Apollo multipart

Only an HTTP `POST` GraphQL JSON request is supported. `Accept` must admit
`multipart/mixed` with `subscriptionSpec=1.0`; quoted/unquoted parameter values
and ordinary optional whitespace are equivalent. If a `boundary` parameter is
present it must be `graphql`. Multipart upload, callback subscriptions,
incremental `@defer`/`@stream`, and other subscriptionSpec values are unsupported.

The 200 response is
`Content-Type: multipart/mixed; boundary=graphql; subscriptionSpec=1.0` and has
no content length. HTTP/1 framing may be chunked by the server; HTTP/2 must not
receive a `Transfer-Encoding` header. Each event part is
`Content-Type: application/json` with `{"payload": <GraphQL response>}`.
Heartbeat parts contain `{}`. Natural completion writes the final `--graphql--`
boundary without inventing a GraphQL result. A fatal setup/source/transport error
that remains writable emits `{"payload":null,"errors":[...]}` and closes the
multipart body. Those top-level transport errors use safe message/extensions
only and omit `locations` and `path`, as required by the pinned profile.
Ordinary GraphQL field errors remain inside `payload` with their normal GraphQL
locations and paths and do not terminate an otherwise live subscription.
Disconnect cancels ownership.

For both HTTP streaming profiles, authentication and decoded hooks run before
source initialization, the complete error/hook table is Section 9.2, and later
event failure cannot change status or headers. Proxy/CDN buffering must be disabled,
idle/read/response timeouts must exceed the configured heartbeat, and application
HTTP-version/listener policy remains outside Chigiri. A body/socket write means
only local transport progress.

### 9.5 Mock binding and publication contract

Each generated subscription coordinate remains one editable resolver module.
It has a `subscribe` entrypoint and, only when the subscription root is reused as
an ordinary nested object, the normal `resolve` entrypoint. Setup returns one
closed binding value; the host validates and copies it, then destroys the setup
VM. Event execution starts a fresh read-only VM/context and uses the published
field-output payload as the root field value before resolving the subscriber's
nested selection.

The generated write context alone exposes the coordinate-specific typed publish
table. The coordinate is not a string and the payload is the generated output
type. This complete example fixes the authoring shape:

```luau
-- resolvers/subscription/presence.luau
--!strict
local resolver: ChigiriSubscriptionPresenceResolver = {
    subscribe = function(
        ctx: ChigiriSubscriptionSetupContext,
        args: ChigiriSubscriptionPresenceArgs
    ): ChigiriSubscriptionPresenceBinding
        return ctx.subscription:bind({
            topic = "team:" .. args.teamId,
            overflow = "drop_oldest",
        })
    end,
    resolve = function(
        ctx: ChigiriReadContext,
        args: ChigiriSubscriptionPresenceArgs,
        parent: ChigiriSubscription
    ): ChigiriPresence
        return parent.presence
    end,
}
return resolver
```

```luau
-- resolvers/mutation/setPresence.luau
--!strict
local resolver: ChigiriMutationSetPresenceResolver = {
    resolve = function(
        ctx: ChigiriWriteContext,
        args: ChigiriMutationSetPresenceArgs
    ): ChigiriPresence
        local result = {userId = args.userId, online = args.online}
        ctx.publish.Subscription.presence({
            topic = "team:" .. args.teamId,
            payload = result,
        })
        return result
    end,
}
return resolver
```

`overflow` is optional and defaults to `"backpressure"`; the only other value
is `"drop_oldest"`. Topic length/content, coordinate, payload type, Record
provenance, phase capability, publication count/bytes, and request budget are
checked statically where representable and again by opaque host capabilities.
`pcall` cannot clear a sticky host failure. Aliasing or forging a table does not
grant publication. Query, hook, setup, and event contexts have no publish table.

The complete request candidate owns the immutable publications. Only its commit
winner transfers them, in call order, to bounded host staging. Rollback and any
pre-commit failure discard them. Registration linearizes only after the binding
has passed setup/admission and entered the server topic index; publications
committed after that point are eligible. No earlier event, replay, publisher
headers/state/time override, retained VM, or candidate store view crosses the
boundary. Each subscriber uses its own current committed snapshot, clock-policy
view, fresh state, hooks, budget, selection, and output validation.
The immutable-root/current-related-record distinction in Section 7 applies to
this reconstruction; a captured Record is not a request to reload its root row.

### 9.6 Closed identity and migration table

The cutover is coordinated and compatibility-free for executable readers:

| Identity | Before | After | Decision and migration |
| --- | --- | --- | --- |
| Generated Rust ABI | `7` | `8` | Regenerate; implement the complete subscription group where present. ABI 7 generated crates fail exact checks. |
| Mock project | `chigiri-mock-project/v8` | `chigiri-mock-project/v9` | Protected update rewrites the project identity and adds subscription modules/inventory. Runtime/check/reference require v9. |
| Portable Mock bundle | `chigiri-mock/v9` | `chigiri-mock/v10` | Re-export from a migrated project; there is no in-place bundle reader. |
| Mock host API | `chigiri-mock-api/v8` | `chigiri-mock-api/v9` | Adds binding/publication and event-context contracts; old API is rejected. |
| Mock capabilities | `chigiri-mock-capabilities/v2` | `chigiri-mock-capabilities/v3` | Adds opaque subscribe/publish capabilities and phase permissions. |
| Full reference | `chigiri-mock-reference/v11` | `chigiri-mock-reference/v12` | Adds generated subscription/binding/publish types, limits, protocols, and lifecycle guidance. |
| Topic reference | `chigiri-mock-reference-topic/v7` | `chigiri-mock-reference-topic/v8` | Carries reference v12/API v9/capability v3 and new topics. |
| Runtime log | `chigiri-mock-log/v3` | `chigiri-mock-log/v4` | Adds bounded subscription setup, publication, drop, and terminal record kinds. |
| Update state | `chigiri-mock-update-state/v9` | `chigiri-mock-update-state/v10` | Owns the new generated module/type inventory and digests. |
| Schema snapshot | `chigiri-mock-schema-snapshot/v9` | `chigiri-mock-schema-snapshot/v10` | Records subscription-root generation inputs and v10 ownership. |

The following identities are explicitly preserved because their serialized
shape/meaning does not change: `chigiri-mock-check/v3`,
`chigiri-mock-directives/v3`, `chigiri-mock-schema-sync/v1`,
`chigiri-mock-sdl-normalization/v1`, `chigiri-mock-luau-format/v1`,
`chigiri-mock-transaction/v1`, and `chigiri-mock-recovery/v1`. New diagnostic
codes and coordinates fit the existing check envelope; no SDL directive is
added; normalization/comparison and transaction/recovery shapes remain the same.

`chigiri mock update --config-dir <project>` is the only command allowed to read
an otherwise valid v8 project for this one-step migration. It first verifies the
old ownership/snapshot/transaction contract, adopted SDL, user files, and exact
v8 identities; then it generates the entire v9/v10 candidate and publishes it
through the existing transaction. It preserves user-owned resolver/hook/storage
files, creates only newly required subscription modules, and reports signature
conflicts without overwriting edits. Interrupted migration leaves the existing
transaction/recovery evidence. Mixed old/new identities are never accepted.

All other new-binary commands reject project v8, bundle v9, API v8, capability
v2, reference v11/topic v7, log v3 input, or management v9 input as applicable.
After project update, run focused `check`, inspect reference, and create a new
portable export. Rust users rerun `chigiri rust codegen`, implement new generated
methods, and rebuild; generated files are never edited. Adopted SDL reconciliation
continues through ADR-0052 with no external fallback.

#### Runtime log v4 records and ownership

The four new kinds use one dedicated stdout queue with the existing log queue
capacity of 64 records, not four queues or a public setting. Hook and access
retain their separate 64-record reservations; the existing eight-entry stderr
notice queue remains isolated. The coordinator rotates access, hook, and
subscription records fairly when stdout is available. The existing
rate-limited loss report remains non-recursive. `log_loss.dropped` has exactly
`hook`, `access`, `subscription`, and `notice`, each a saturating `u64` cumulative
loss count. Shutdown discard and unconfirmed writes charge the original class.
The existing isolated writer processes, 16-KiB record ceiling, one unacknowledged
frame per sink, and original shutdown deadline are unchanged. Subscription logs
never wait for output, fail an operation, alter delivery, or consume hook/access
reserved slots. A producer's enqueue attempt is not successful log delivery.

Each new record contains the existing envelope fields `format`, `timestamp`,
and `request_id`, followed by `kind` and the fields below. `format` is exactly
`chigiri-mock-log/v4`; `timestamp` is a signed JSON integer of UTC epoch
milliseconds. Every listed field is present, including explicit JSON nulls.
`request_id` retains the existing Mock request-ID number representation and
allocation rules; this cutover does not introduce a different request-ID limit.
Existing record kinds retain their field meanings, apart from the new format
and the additional subscription loss class.

| Kind | Required kind-specific fields | Validity and correlation |
| --- | --- | --- |
| `subscription_setup` | `subscription_id`, `coordinate`, `overflow`, `outcome` | Non-null fresh setup `request_id`. `outcome` is `registered`, `failed`, or `cancelled`. Registered requires a non-null subscription ID and `backpressure` or `drop_oldest`; failed/cancelled require both ID and overflow to be null. |
| `subscription_publication` | `sequence`, `index`, `coordinate`, `outcome` | Non-null publisher Mutation `request_id`. `sequence` is the existing commit sequence; `index` is the zero-based publication call index, less than the captured batch length. `outcome` is `staged` or `lost`. |
| `subscription_drop` | `subscription_id`, `coordinate`, `reason`, `dropped_total` | Null `request_id`. `reason` is only `drop_oldest`; the total is the positive post-increment cumulative count. |
| `subscription_terminal` | `subscription_id`, `coordinate`, `outcome`, `cause`, `dropped_total` | Null `request_id`. The total is frozen at the logical terminal decision. Only the outcome/cause pairs below are valid. |

Terminal pairs are `completed/source_complete`, `failed/execution_failed`,
`failed/delivery_failed`, `failed/internal`, `cancelled/client_cancelled`,
`cancelled/transport_closed`, and `cancelled/server_shutdown`. Source or event
execution failure is not successful completion. Cause selection uses typed
execution/controller/cancellation state, never GraphQL error text, a wire close
string, or protocol-specific inference. The winning logical terminal decision
fixes the pair once; cleanup or failed terminal-frame transmission cannot
replace it. An otherwise unconsumed observation owner reports `failed/internal`
once rather than inventing a new cause or claiming completion.

One common mapper consumes the following closed typed decision classes for
both HTTP and WebSocket. Protocol adapters forward the typed decision; they do
not select an outcome/cause themselves.

| Winning typed decision | Outcome/cause |
| --- | --- |
| Natural source end, with no winning failure/cancellation | `completed/source_complete` |
| Source-item failure; event preparation, policy, hook, projection, or execution failure; execution/context budget or phase deadline | `failed/execution_failed` |
| Delivery queue age/capacity, encoded-event size, send-progress, shared encoded-byte pressure, or drop-counter exhaustion | `failed/delivery_failed` |
| Registration-to-activation ownership/port failure; worker/runtime/invariant/poison failure; an otherwise unconsumed decision owner | `failed/internal` |
| Explicit client operation completion or unsubscribe while the transport is usable | `cancelled/client_cancelled` |
| Peer disconnect, unusable transport, or transport/protocol closure without a previously winning shutdown/cancellation/failure | `cancelled/transport_closed` |
| Explicit Mock server shutdown/registration close selected by the application | `cancelled/server_shutdown` |

A selected initial-HTTP overall/setup deadline belongs to execution, not
delivery. Before registration it selects setup `failed` and no terminal; after
registration it selects `failed/execution_failed`. Upgrade/initialization
failures before any Mock dispatch create no subscription records. A runtime
output/completion budget failure is execution failure; bounded wire encoding
or outbox-size failure is delivery failure. A business error retained as an
ordinary event is neither. A known application/policy failure after registration
uses execution failure even if activation cannot finish; an unexplained broken
activation handoff uses internal failure. Unknown bare cancellation is internal
fallback, not an invented client request.

These rows classify the existing winning logical decision, not a new priority
order. Original deadline-first checks and controller/cancellation linearization
remain authoritative in simultaneous-ready races. Once selected, the pair and
final total survive subsequent disconnect, shutdown, quarantine, late result,
cleanup failure, and owner Drop unchanged. Quarantine and physical cleanup are
not additional terminal causes. A fallback may win only while no decision has
already been selected. Fixtures must exercise every row and retain a known
winner through competing terminal attempts on both HTTP and WebSocket.

`subscription_id` is the existing Mock topic-index generation, not a peer
WebSocket ID, request ID, topic, model key, or process-global identity. It is
positive, server-local, and never reused within that server. Restart promises
no continuity. The generation, commit sequence, and delivery total retain the
existing exact JSON integer ceiling `2^53 - 1`; publication indices are bounded
by the existing request publication count. No logging constructor may change
the underlying sequence/counter exhaustion behavior, wrap, clamp, or allocate a
second identity sequence. Existing log-loss counters retain their separate
saturating `u64` contract. Bounded log loss permits gaps, and concurrent producer
enqueue order does not add a global cross-request log ordering guarantee.

The once points and owners are fixed as follows:

- The outside setup decision owner has one enqueue attempt for each source
  initializer that reaches Mock dispatch. Successful topic-index registration
  selects `registered`. A setup
  failure or cancellation before registration selects the other outcomes at its
  logical decision without waiting for physical cleanup. Pre-validation or
  context failures that never invoke Mock setup produce only existing records.
  Registration followed by activation failure produces a registered setup and
  one terminal record, never a second failed setup record.
- Publication has one enqueue attempt per committed item, in captured call
  order, only after the store commit and after store/topic/cancellation locks
  are released. `staged` means the retained batch entered bounded host staging;
  it is not a subscriber, transport, or peer receipt. `lost` means the existing
  post-commit loss path was selected. Rollback, pre-commit failure, and requests
  without publications produce no publication records. A bounded handoff may
  temporarily share the existing retained batch, but queued captures never gain
  a Logger, request metadata, store, IR, VM, or credentials.
- The common delivery controller owns drop observations. Each successful
  whole-unsent eviction and successful counter increment selects exactly one
  enqueue attempt, outside its lock. Backpressure, failed eviction, a partial
  frame, and counter exhaustion produce no drop record. Protocol writers do
  not synthesize counts. Drop and terminal decisions share the controller's
  timeline, so no eviction can follow the frozen terminal total.
- Each successfully registered generation has exactly one terminal enqueue
  attempt at logical revocation, before physical cleanup. It neither waits for
  nor asserts terminal-frame transmission, source destruction, worker join,
  or log flush. Client cancellation, transport loss, shutdown, timeout,
  quarantine, late completion, and owner Drop must not duplicate it. Existing
  `request_terminal` remains the separate once-per-fresh-request phase record,
  including setup/event phases; it is not a subscription lifetime record.

Before source-initializer application or VM work can run, the existing outside
HTTP supervisor or WebSocket operation owner must hold a capacity-one hidden
setup decision authority. It contains only the fresh setup request ID, shared
schema coordinate, bounded sink, dispatch-entered state, and one closed decision
slot. Allocate/check its metadata under the existing pending-setup and retained
metadata bounds; it adds no task, queue capacity, public setting, generation,
or reusable execution authority. After fresh context creation, hand this
authority to the outside owner through the existing bounded setup handoff and
confirm that transfer before entering initializer work. If context creation
never finishes or dispatch is short-circuited, no dispatch-entered transition
occurs and no setup record is emitted.

Dispatch marks entry before invoking its initializer. Entry, outside failure/
cancellation, and registration use the existing ordered registration/
cancellation fence with one fixed lock order. A decision closes entry and
registration atomically: if cancellation wins before entry, delayed work cannot
enter; if it wins after entry but before registration, the outside owner selects
one null-ID/null-overflow failed or cancelled record and late work cannot
register. Timeout, execution failure, unexplained unwind, or broken handoff
select `failed`; explicit client cancellation, transport closure, and server
shutdown select `cancelled`. No user work runs inside that fence.

If registration wins, its existing generation and policy replace the slot with
the registered decision in that same short fence. A racing outside cancellation
then addresses the registered generation and selects its terminal record, never
a failed/cancelled setup. The outside owner alone consumes decision data and
performs enqueue attempts after all locks are released. It consumes registered
setup before terminal, even if the initializer has not returned or a terminal
wins before the outside owner's next poll. The slot holds fixed decision data,
not encoded records or application values; registered setup and subsequent
terminal are drained in order without an additional queue. Source task Drop,
quarantine, late result, and cleanup can signal the same authority but cannot
retry or change a consumed decision. This authority becomes the registered
generation's observation owner without creating a second lifecycle identity.
The deterministic fixture set must hold initializer/VM work blocked after entry
and cover cancellation, execution timeout, shutdown, unwind, both registration
race orders, cancellation before entry, late registration/result rejection, and
registered-then-terminal enqueue order before the blocked work is released.

Mock attaches the bounded sink at its server scope. Carry only one closed
internal observation owner for a registered generation through erased source,
runtime session, and common HTTP/WebSocket delivery ownership. Its identity is
issued by registration and contains only the generation and shared schema
coordinate, with retained metadata/owner bytes checked before registration.
It carries no topic, VM, application value, execution authority, or publisher
request. Cross-crate plumbing may use the existing hidden runtime integration
boundary, but must not expose a generic observer, arbitrary callback, builder
setting, SDK, or user-forgeable Mock identity. Ordinary Rust sources have no
Mock observation owner. Register/commit code returns bounded observation data;
it never serializes, enqueues, calls user code, or destroys user values under
store, cancellation, delivery, or topic locks. A blocked source task must not
delay logical terminal observation in its outside owner.

Automatic subscription records contain no topic strings, payloads, responses,
error messages, query/document/body, variables, extensions, operation names,
peer IDs, headers/cookies, init data, credentials, hook state, model keys,
digests/private fields, or provider/source text. Coordinates come only from
schema metadata; outcomes/causes are closed safe strings. Explicit `ctx.log`
remains user-controlled and is not made secret-safe by this redaction contract.

Exact version-controlled fixtures must cover all shapes, nullable combinations,
terminal pairs, sequence/counter bounds, rollback versus post-commit loss,
successful drop versus partial-frame exclusion, cancellation/unwind/activation
races, terminal-before-cleanup, and diagnostic privacy markers. Queue fixtures
must cover exact 16-KiB/newline boundaries, full/closed/broken sinks, fair
three-class progress, notice isolation, and shutdown loss attribution. Real Mock
wire fixtures correlate setup/publication/drop/terminal without treating them
as delivery receipts. There is no log ingestion API: old/mixed log rejection
uses existing reference consumers and exact serializer fixtures where applicable,
not an invented runtime log reader. These requirements are the preparation
contract, not executable evidence by themselves. Current serializer, private
host, common delivery and real-wire evidence is recorded in the dedicated
validation record; final whole-goal acceptance remains separate.

### 9.7 Implementation ownership and focused proof plan

The core owns GraphQL validation and a transport-neutral executable subscription
plan. Runtime owns generated source/event dispatch and sequential event execution.
The facade owns immutable configuration, endpoint-clone quotas, retained request
metadata, common evaluator, reservation tokens, cancellation, and delivery state
machines. HTTP owns neutral request/profile/framing values. Axum and Hyper own
only library upgrade/body/socket adaptation. Mock owns typed bindings,
transaction candidate publication, post-commit staging, and fresh VM/store/clock
creation. No bridge or Mock worker may reimplement limit decisions.

Before SU-WP1, compile tests must prove the complete generated group, associated
event/output types, one mapper call, nested-root separation, owned `'static`
source, and rejection of borrowed sources, `Send`-only/non-`Sync` events, and
incomplete groups. A positive event fixture proves the matching `Send + Sync`
contract. Runtime tests use
deterministic sources and clocks for setup-once, sequential fresh contexts,
argument reuse, plugin/selection/completion behavior, every failure phase, and
cancellation drops. Evaluator table/property tests cover each condition alone
and combined, arithmetic boundaries, reservation transfer, counter precision,
whole-entry eviction, timers without new input, fairness, and permit release.

SU-WP3 uses small real Hyper and Axum applications for each pinned protocol,
including negotiation, authentication inputs, setup/event errors, multiple
events, cancellation, heartbeat, malformed/oversized controls, multiplexed
isolation, stalled sinks, and concurrent Query/Mutation/Subscription admission
on WebSocket. Init fixtures cover accepted/rejected/internal/timeout outcomes,
credential visibility for all operation phases, and cross-connection/generation
non-leakage. The HTTP fixtures exercise every Section 9.2 table row through both
bridges. Repeated heartbeat expiry behind a partially progressing event proves
one-marker coalescing, event priority, byte reservation failure, and cleanup. A
deterministic WebSocket race completes an ID, immediately reuses its text while
old cleanup is still blocked, and proves no 4409, stale emission, new-owner
deletion, or early resource release. A dedicated-worker fixture holds a
synchronous source poll past the five-second cleanup wait, observes charged
quarantine/capacity denial, then releases it and proves one late token release.
Multipart fixtures distinguish ordinary payload errors, which retain GraphQL
locations/paths, from top-level transport errors, which omit both. SU-WP4 uses
real Mock projects for typed bind/publish, commit/rollback, order/registration,
sticky limits, permission forgery, private provenance, per-subscriber selection
and state, clock changes, worker recovery, restart, and seed reset. Exact-
boundary/one-byte-over publication captures cover every scalar/container/
provenance charge and reservation transfer. Deterministic full-queue tests cover
registration immediately before/after a lost commit, two overlapping losses
with a registration between them, terminal preemption, unaffected topics/
servers, topic-entry removal/recreation, and later admission. Event fixtures
cover the reserved delivery-extension path, sibling preservation, forged-leaf
replacement, and both non-object prefix collisions. SU-WP5 runs the documented
Rust/Mock/Apollo examples and the frozen migration matrix.

Evidence belongs in the dedicated validation record and Beads. Tests use these
normative contracts and version-controlled fixtures, never Beads status. The
independent SU-WP0 architecture/security review must resolve and verify every
finding before executable work starts. This freeze is not permission to extend
scope, vendor a dependency, run a load campaign, or alter Query/Mutation HTTP
behavior.

## 10. Acceptance and evidence

All rows remain required assertions. The validation record maps executed
source-bound evidence and remaining gates; a row is not passed merely because
its fixture or a component review exists. Map them to
focused tests in [Conformance Section 41](../spec-conformance.md#41-subscription-and-bounded-delivery-conformance).
Follow [Handoff Section 41](../implementation-handoff.md#41-subscription-and-bounded-delivery-work-packages).

| ID | Required evidence |
| --- | --- |
| SU-01 | Positive/negative subscription validation, typed source and output compilation, complete groups, nested root behavior, selection, coercion, aliases, abstract output, and null propagation. |
| SU-02 | Setup runs once; each event gets fresh context/cache/state/identity/budget, deliberate shared services, current credential and root/argument authorization, and bounded preparation. No source borrows discarded setup state. |
| SU-03 | Setup rejection, event field errors, context/source/terminal failures, masking, exactly-once error mapping, header-hook timing, and safe observable termination remain distinct. |
| SU-04 | Sugar/inject equivalence, accepted defaults/hard maxima, call-order replacement, immutable effective configuration/origins, and shared endpoint-clone accounting. Valid boundary values construct; over-maximum, zero, overflow, and inconsistent settings return construction errors without panic, clamping, or deferred startup failure. |
| SU-05 | Each limit alone and in combination obeys OR semantics; capacity eviction and expiry recheck all conditions; shared encoded-buffer pressure, oversized events, stalled sends, and atomic concurrent admission take their specified paths. Deterministic clocks and stalled sinks prove timers act without new input. |
| SU-06 | Default demand/backpressure never silently drops; explicit server-only DropOldest evicts whole unsent events in order, counts accurately across count/byte/age pressure, preserves fresh cumulative metadata, and never evicts control or partial messages. |
| SU-07 | Cancellation, disconnect, shutdown, rejected admission, and terminal failure release source/task/worker/permit/queue ownership. Multiplexed failure preserves unrelated operations unless the shared transport is unusable. |
| SU-08 | Real small Rust consumers exercise all three transports through both official bridges: start, multiple events, errors, cancellation, and bounded slow-consumer behavior. Record protocol/client versions. |
| SU-09 | Real Mock consumers exercise all three transports, typed explicit publish after commit, ordering/registration, rollback/no-publication, subscriber isolation, fresh state/store/clock views, private provenance, VM recovery, restart, and seed reset. |
| SU-10 | Oversized/malformed protocol and publish input, cross-subscriber/cross-server leakage, forged records, permission bypass, exhausted staging, and stalled peers fail safely within resource bounds. |
| SU-11 | Documented Rust/Mock and Apollo metadata recipes execute; no SDL-internal loss fields or extra client SDK is required. Protected generation, adopted snapshots, reference, portable export, version rejection, and migration match the frozen cutover table. |
| SU-12 | Focused affected tests, MSRV/stable checks where applicable, exact generation, Clippy/rustdoc/formatting, implementation rules, and scoped independent review evidence identify the actual source and unexecuted scope. |

Use small local macOS applications and bounded deterministic unit/integration
checks, not elaborate production-load simulations. Formal throughput, load,
soak, fuzz, remote Linux, cross-build, and full-product validator campaigns are
not mandatory for this assignment. Product support tiers remain unchanged;
local success does not qualify other platforms or certify production capacity.

Track implementation and all new reviews/findings in Beads under the
[review workflow](../review-workflow.md). Implementers record Fixed; independent
reviewers record Verified and close findings. Validation records contain
supporting evidence, not another review ledger. Do not describe this document's
publication, illustrative code, or a previous source's tests as SU passes.

## 11. Consequences and exclusions

Fresh per-event preparation and execution may cost more than retaining a setup
context. That explicit cost is accepted. A bounded producer/consumer pipeline,
three protocol integrations, and post-commit Mock events are substantive work,
not merely JSON framing. Prefer a clear, safe first implementation to caching,
deduplication, or parallel event execution that obscures ownership.

Applications own event meaning, durable logs, cursors, replay/resumption,
authorization, and any gap-free initial-state protocol. Chigiri promises no
exactly-once delivery, automatic retries, client acknowledgements, broker,
cluster-wide quota, persistent queue, or transparent recovery. DropOldest fits
replaceable state updates, not arbitrary deltas or financial history.

Do not add Mock automatic model notifications, timers/scheduled scripts,
external networking, shared Luau modules, a scenario-test runner, hot reload,
an output-mapping DSL, or a standalone Rust server as incidental work.

## 12. Protocol references

- [GraphQL September 2025: subscription execution](https://spec.graphql.org/September2025/#sec-Subscription),
  [single root field](https://spec.graphql.org/September2025/#sec-Single-Root-Field),
  [delivery agnostic](https://spec.graphql.org/September2025/#sec-Delivery-Agnostic),
  and [extensions](https://spec.graphql.org/September2025/#sec-Extensions).
- [GraphQL-over-WebSocket protocol](https://github.com/enisdenjo/graphql-ws/blob/master/PROTOCOL.md).
- [GraphQL-over-SSE protocol](https://github.com/enisdenjo/graphql-sse/blob/master/PROTOCOL.md).
- [Apollo multipart subscription protocol](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/multipart-protocol).
- [Apollo Client subscription integration](https://www.apollographql.com/docs/react/data/subscriptions).

Moving protocol links above are discovery references, not a floating
implementation contract. SU-WP0 must record the exact revisions/profiles and
client versions used by implementation and acceptance.
