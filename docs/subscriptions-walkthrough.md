# Subscription walkthroughs

This guide points to the small executable consumers for Chigiri's subscription
contract. It does not replace the normative architecture or ADRs. The examples
use only the supported WebSocket `graphql-transport-ws`, distinct-connection
SSE, and Apollo multipart subscription profiles.

## Execution tracing

The [distributed tracing guide](distributed-tracing.md) uses one optional
Endpoint registration for setup and all event work. Setup adopts its incoming
parent and ends promptly; recorded events have fresh roots and setup links.
Every event preserves the captured standard suppression boolean, including
without a valid parent. Source waiting retains bounded values without a live
setup span. Event-local suppression does not change later events' baseline.

The [maintained generated consumer](../crates/testkit/tests/usability/distributed_tracing/subscription_events.rs)
uses C=(), ordinary traits and all six native adapter/profile combinations.
The existing stream payload and delivery-counter contract is unchanged.
Applications still own downstream client propagation, publisher instrumentation
and telemetry-provider shutdown; SDK span completion is not peer receipt.

## Typed Rust resolver assembly

The complete ABI 8 consumer is
[`crates/codegen/tests/compile/subscription.rs`](../crates/codegen/tests/compile/subscription.rs).
It demonstrates one generated resolver group containing:

- `SchemaTypes`, the query group, the subscription group, and the nested
  `PresenceResolver` group;
- an application `FieldError` mapper supplied once to `Resolvers::builder`;
- `subscribe_presence`, which borrows request and argument data only during
  setup and returns an owned `SubscriptionSource`;
- `resolve_presence_event`, which maps each owned source item into its root
  result; and
- the ordinary `Subscription.presence` method used when a nested selection
  reaches the subscription root through a non-subscription operation.

Run the exact generated-consumer check with:

```console
cargo +1.95 test --locked --offline -p chigiri_cli --test rust_generation generation_subscription::subscription_group_compiles_complete_and_rejects_invalid_contracts -- --exact
```

The six supported adapter/profile combinations have a real Rust consumer in
[`crates/testkit/tests/subscription_streaming.rs`](../crates/testkit/tests/subscription_streaming.rs).
Its bounded local harness is executable with:

```console
cargo +1.95.0 test -p chigiri_testkit --test subscription_streaming --locked --offline official_services_drive_all_profiles_over_owned_loopback_connections -- --exact --test-threads=1 --nocapture
```

These fixtures are the source examples: copying their generated traits into a
guide would let documentation drift away from the checked ABI.

## Native routes and optional shutdown

The small runnable [Axum application](../examples/subscriptions/README.md)
serves finite Query and POST SSE through one application-owned listener. It
uses a dedicated, protected ABI 8 contract and complete resolver traits. Its
owned source shares immutable domain backing with `Arc`; source setup,
event-to-root mapping and selected-field resolution are separate methods.
It also checks exact route/Origin rejection before setup and source cancellation
on disconnect. The application's source has bounded state and creates no
producer task; replay/catch-up/watch coordination is outside this minimum.

Register the public managed service inside the existing Tokio runtime:

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

Service construction starts shared SDK supervision fallibly. Clones share
owners and quotas; the SDK owns finite execution, source/event work, timers,
cancellation, completion and bounded recovery. Hyper uses
`chigiri_hyper::managed_service(graphql.clone())?` at its native service boundary.
Applications own listeners, runtimes, routes, origins, connection admission,
signals, TLS and HTTP versions. No manual GraphQL driver, per-upgrade owner,
server extension or final process-reaper call is required.

Omitting `.with_graceful_shutdown(...)` is supported. Last service-owner release
requests cancellation, but routing alone provides no coordinated drain or
knowledge of when native shutdown began. A long-lived source can therefore
delay the native server's shutdown without an SDK notification.

`ShutdownOptions::graceful()` has one 30-second maximum SDK wait.
`ShutdownOptions::graceful_for(Duration::from_secs(5))?` provides a typed
positive override; zero/unbounded/unschedulable values are rejected.
`ShutdownOptions::immediate()` instead requests cancellation without waiting
for workers or preempting arbitrary Rust code. It does not roll back committed
writes. Graceful stops new admission/demand, finishes eligible finite/active
event work under existing shorter limits and skips unstarted backlog rather
than waiting for natural source completion. It never adds a cleanup wait after
expiry, and physical owners remain charged until actual release.

The SDK drains or classifies failure/expiry before its future allows native
server shutdown. Other routes and TCP admission can remain available meanwhile.
The duration does not bound later server-wide shutdown or establish peer receipt.
Optional `graphql.shutdown_outcome()` distinguishes drain, cancellation request,
expiry and infrastructure failure without another required await. A late
physical join does not rewrite failure or expiry. The maintained application
and public native-hook tests exercise these distinctions.

## Whole-configuration sizing

An encoded-event limit counts the complete Chigiri profile event. Start with
the selected GraphQL result: aliases, data, errors and application extensions,
then UTF-8 JSON serialization/escaping, Chigiri delivery metadata (including
its reserved 16-byte counter value slot), and profile framing. SSE includes
its lines; multipart includes boundaries/part headers/envelope; WebSocket
includes the GraphQL envelope and escaped operation ID. This framing is already
inside `max_encoded_event_bytes`; do not add it twice.

Rust/domain heap size and raw message byte length are different measurements.
Quotes, backslashes and control characters expand in JSON; UTF-8, errors,
selected fields and operation IDs vary. No fixed multiplier on raw DTO bytes
bounds every result. Test representative maximum selections and content for
each profile. Transport-library WebSocket frame headers, HTTP chunk/HTTP2
framing, TLS, OS/socket/proxy/client buffers, upstream queues and application
domain heaps are outside the encoded-event budget.

The following examples use binary KiB/MiB and the existing complete settings:

| Setting | Default example | Larger-event example |
| --- | ---: | ---: |
| Active subscriptions `S` | 128 | 8 |
| Queued-event count cap | 32 | 4 |
| Per-subscription queued/in-flight bytes `Q` | 262,144 (256 KiB) | 2,097,152 (2 MiB) |
| Maximum candidate event `E` | 65,536 (64 KiB) | 786,432 (768 KiB) |
| Shared encoded budget `B` | 67,108,864 (64 MiB) | 33,554,432 (32 MiB) |
| Conservative event allowance `S * (Q + E)` | 41,943,040 (40 MiB) | 23,068,672 (22 MiB) |
| Remaining shared headroom `B - allowance` | 25,165,824 (24 MiB) | 10,485,760 (10 MiB) |

The larger settings replace one complete value; unspecified fields retain
their unchanged defaults:

```rust,ignore
let limits = chigiri::SubscriptionLimits {
    max_active_subscriptions: 8,
    max_queued_events: 4,
    max_queued_bytes: 2 * 1024 * 1024,
    max_encoded_event_bytes: 768 * 1024,
    max_endpoint_encoded_bytes: 32 * 1024 * 1024,
    ..chigiri::SubscriptionLimits::default()
};
let graphql = chigiri::Endpoint::builder(resolvers)
    .subscription_limits(limits)
    .without_context()
    .build()?;
```

One event must fit both `Q` and `B`. Byte and event-count caps are independent:
the queue need not hold `count_cap * E`. `Q` includes queued and in-flight event
entries; the event-count cap excludes the one in-flight entry. Default `Q`
holds four maximum-size entries in total, for example one in-flight and three
queued. Larger-example `Q` holds two, for example one in-flight and one queued:
they occupy 1.5 MiB, while three exceed `Q`. Four maximum-size queued entries
are not promised by its count cap. Smaller entries can use more count slots.

One `E` reservation is acquired before source demand, then shrinks to actual
encoded bytes. It transfers with that event, rather than becoming another
queued entry or being counted twice. `S * (Q + E)` conservatively plans for
event storage and one candidate per subscription; it is not a construction
inequality or a complete endpoint-memory bound. Compute addition,
multiplication and remaining headroom with checked arithmetic. The tests cover
both addition and multiplication overflow rather than wrapping a product.

The stated headroom stays in the existing shared budget for initial/setup
responses, controls, heartbeats, terminal messages and physical retained owners.
Bytes already charged within `Q` are not added again merely because cleanup is
pending. Other active/control concurrency still needs a workload-specific plan;
a valid graph can pause or terminate under shared pressure, and individual hard
maxima do not promise every subscription its maximum simultaneously.

The [checked sizing fixture](../crates/testkit/tests/usability/subscription_sizing.rs)
uses the bounded production encoder with escaped UTF-8/control characters,
an alias, an error, application extensions and delivery metadata. Its WebSocket
case includes a 256-byte escaped operation ID. For this exact fixture:

| Profile | Included overhead beyond raw message bytes | Raw message at 64-KiB encoded limit | Raw message at 768-KiB encoded limit |
| --- | ---: | ---: | ---: |
| WebSocket | 598 bytes | 64,938 bytes | 785,834 bytes |
| SSE | 325 bytes | 65,211 bytes | 786,107 bytes |
| Multipart | 364 bytes | 65,172 bytes | 786,068 bytes |

These overheads describe only that version-controlled result/ID, not a general
estimator. Each encoded event fits its limit exactly with null, zero and maximum
safe-integer counters at the same 16-byte reserved width. Reducing the limit
or increasing the message by one byte fails. The fixture also builds both
configuration graphs and checks the table's arithmetic. It does not measure
network delivery or production capacity:

```console
cargo +1.95.0 test --locked --offline -p chigiri_testkit --test usability subscription_sizing -- --test-threads=1 --nocapture
```

## Consumer workspace and producer identity

The [application README](../examples/subscriptions/README.md#generated-contract-and-workspace)
shows the standalone generated `[workspace]`, consumer member and explicit
generated-root exclusion. Keep the generated crate separate from handwritten
application code and build caches; both sides must use the same Chigiri source.
Inspect `chigiri rust --version --format json`, the generated marker's producer
build/revision and ABI 8, then run protected `rust check` before compiling.
Compilation alone does not prove generation freshness. Use the canonical
producer's protected `update` for deliberate changes; generated bytes and
ownership hashes are never edited by hand. The checked example records its
current local dirty producer identity honestly. License/dependency-version
metadata remains a separate packaging candidate under Beads `lab01-qke`.

## Mock binding and publication

A Mock subscription root binds to a bounded topic source. The existing
component fixture constructs this checked Luau module through the normal Mock
renderer:

```lua
return ctx.pubsub.subscribe.Item({
    topic = args.topic,
    overflow = "drop_oldest",
})
```

The fixture declares the related-record read explicitly:

```graphql
type Item {
  relatedId: ID!
  related: Related! @relation(fields: ["relatedId"], references: ["id"])
}
```

Only a successful mutation may publish. The captured `row` is the immutable
event root even if the mutation changes that record and its relation later:

```lua
local relatedId = "related-" .. args.id
ctx.models.Related:create({ id = relatedId, label = "captured-" .. args.id })
local row = ctx.models.Item:create({
    id = args.id,
    value = args.value,
    relatedId = relatedId,
})
ctx.pubsub.publish.Item({ topic = "events", payload = row })

ctx.models.Item:update(
    { where = { field = "id", op = "eq", value = args.id } },
    { value = args.value + 100 }
)
ctx.models.Related:update(
    { where = { field = "id", op = "eq", value = relatedId } },
    { label = "current-" .. args.id }
)
```

For a declared record relation selected below that root, each event worker
reads a fresh request snapshot. The executable fixture therefore proves both
properties together: a subscriber sees the captured root value while
`related.label` resolves to the later `current-<id>` value. A second event
retains its own captured root and resolves its own relation from a fresh
snapshot. No captured root is rebound to the current row.

The actual product listener, all three wire profiles, publication order,
relation freshness, cancellation, source failure, and recovery are covered by
one existing focused CLI acceptance test:

```console
cargo +1.95 test --locked -p chigiri_cli --test mock_cli subscriptions::mock_subscriptions_cross_all_three_real_wire_protocols_and_recover -- --exact --nocapture --test-threads=1
```

The smaller component source is in
[`crates/mock/src/service/dispatch/subscription_tests`](../crates/mock/src/service/dispatch/subscription_tests).

## Observing delivery metadata with Apollo Client

Ordinary application code should continue to consume subscription `data` and
GraphQL errors normally. When an advanced client needs Chigiri's cumulative
drop counter, place the non-terminating
[`deliveryMetadataLink`](../tests/fixtures/subscription-recipes/apollo/delivery-metadata-link.mjs)
before the terminating transport link:

```js
import { ApolloLink } from "@apollo/client/core";
import { deliveryMetadataLink } from "./delivery-metadata-link.mjs";

const link = ApolloLink.from([
  deliveryMetadataLink(({ operationName, droppedTotal }) => {
    deliveryCounters.set(operationName, droppedTotal);
  }),
  transportLink,
]);
```

The callback receives only canonical `null` or non-negative safe-integer values
from `extensions.chigiri.delivery.droppedTotal`. The link returns the original
result object, including ordinary data, GraphQL errors, and sibling extensions.
It also preserves source errors, completion, and unsubscription. This is a
Link-layer observation recipe; it does **not** promise that React
`useSubscription` exposes arbitrary response extensions.

Execute the isolated recipe with:

```console
./scripts/check-subscription-recipes.sh
```

The fixture pins `@apollo/client` 4.3.5 (git commit
[`f3d28a55e5996429a57bf5367187bf8991464a89`](https://github.com/apollographql/apollo-client/tree/f3d28a55e5996429a57bf5367187bf8991464a89)),
`graphql` 16.14.2, and `rxjs` 7.8.2. Its lock file records Apollo's published
SHA-512 integrity value
`sha512-phA1B82EisAHIjT6irj9Lm4I5516pASExgACUl8pzcnq5eGmNjBhImDgjSRHRt18u1Hrqw0uP71jKJtEjV/Fag==`.
The recipe follows Apollo's documented
[`forward(operation)` Link chain](https://www.apollographql.com/docs/react/api/link/introduction)
and the pinned
[`ApolloLink.execute` contract](https://github.com/apollographql/apollo-client/blob/f3d28a55e5996429a57bf5367187bf8991464a89/src/link/core/ApolloLink.ts).

## Bounded delivery and cleanup

Chigiri accounts its retained subscription metadata, pending setup, internal
encoded-event queues, in-flight frames, and control queues. Those bounds do not
account for an application's upstream domain source, operating-system socket
buffers, a proxy, or a client's own buffers; applications must bound those
separately.

The default `Backpressure` policy does not evict a queued event, but bounded
pressure can pause source demand or terminate a source that cannot make timely
progress. Server-only `DropOldest` removes only whole, unsent event results. It
does not evict control or terminal frames, partially accepted frames, or
already in-flight bytes. Every event result reports the cumulative counter:
`null` when dropping is unavailable, otherwise a non-negative integer.

Disable unwanted proxy response buffering and align proxy idle timeouts with
the application's heartbeat policy. Chigiri cannot configure a proxy or make
its buffering bounded. Heartbeat traffic keeps an otherwise idle connection
observable. It is not an
acknowledgement, replay cursor, or proof that a peer consumed an event. Local
write acceptance likewise is not peer receipt. Chigiri provides no implicit
replay or reconnect guarantee.

Client cancellation and server revocation release the logical operation ID
promptly. Source cancellation is cooperative. A source future, event worker,
or physically retained output frame may outlive that logical release while its
bounded cleanup owner is driven or quarantined. Quota and byte charges are
released only when the corresponding physical owner is finally dropped.

Fresh event preparation rechecks the original subscriber metadata and retained
root arguments against current authorization. Revocation is observed on the
next preparation/policy check, not by an automatic external credential watcher;
different credentials require a new subscription. Initial header hooks do not
rerun for each event. See [Rust context guidance](rust-integration.md#typed-subscription-sources-and-application-owned-streaming).

## Evidence scope

The Apollo recipe was executed locally with Node 24.15.0 and npm 11.12.1. The
Rust and Mock commands above name the already executed, version-controlled
focused harnesses; they are intentionally not hidden inside the Apollo recipe
script. The walkthrough does not qualify a browser or React render lifecycle,
an external proxy or CDN, remote Linux deployment, arbitrary ecosystem clients,
network failure after peer receipt, load, soak, or fuzz behavior.
