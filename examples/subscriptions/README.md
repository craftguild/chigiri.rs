# Native Axum subscriptions

This maintained ABI 8 application serves a finite Query and POST SSE through
one application-owned Axum Router and listener. The application owns its runtime,
routes, exact Origin allowlist, signal and source. Chigiri owns managed execution,
supervision, deadlines and recovery. There are no manual GraphQL drivers,
per-upgrade owners, hidden SDK imports or final process-drain calls.

From the repository root:

```console
cargo +1.85.0 run --locked --offline -p chigiri_subscriptions_example
```

In another terminal:

```console
curl -sS http://127.0.0.1:4000/graphql -H 'Content-Type: application/json' -H 'Origin: http://localhost:3000' --data '{"query":"{ current { number message } }"}'
curl -N http://127.0.0.1:4000/graphql -H 'Content-Type: application/json' -H 'Accept: text/event-stream' -H 'Origin: http://localhost:3000' --data '{"query":"subscription { readings { number message } }"}'
```

The source emits eight readings at 500-ms intervals. Closing the SSE client
while it is active cancels its owned source. `/wrong` returns 404; an Origin
other than the exact `http://localhost:3000` allowlist entry returns 403 before
source setup. Non-browser requests without Origin are allowed. This example
does not implement credentialed CORS or an application authentication system.

## The small integration

[`src/lib.rs`](src/lib.rs) builds the typed endpoint and the application Router:

```rust,ignore
let app = axum::Router::new()
    .route_service("/graphql", chigiri_axum::service(graphql.clone())?);
```

The actual Router also applies the Origin middleware before that service and
provides `/health`. Service construction is fallible and happens inside the
existing Tokio runtime. A minimum without a coordinated shutdown hook is
`axum::serve(listener, app).await?`; releasing the last service owner requests
SDK cancellation. The SDK cannot infer native server shutdown notification
from route registration, and long-lived sources can delay native shutdown.

[`src/main.rs`](src/main.rs) adds the standard Axum shutdown API:

```rust,ignore
axum::serve(listener, app)
    .with_graceful_shutdown(graphql.shutdown_on(signal, options))
    .await?;
println!("SDK shutdown: {:?}", graphql.shutdown_outcome());
```

The application supplies `signal`. The executable uses Ctrl-C and the default
Graceful policy; inspection after serving is optional and adds no SDK await.

| Policy | Public expression | SDK behavior |
| --- | --- | --- |
| Default Graceful | `ShutdownOptions::graceful()` | One maximum wait of 30 seconds. |
| Typed override | `ShutdownOptions::graceful_for(Duration::from_secs(5))?` | One valid positive SDK duration; zero and unschedulable durations are rejected. |
| Immediate | `ShutdownOptions::immediate()` | Close admission and request cancellation without waiting for workers. |

Graceful stops new GraphQL admission and source demand, finishes eligible
in-flight work under existing shorter deadlines and skips unstarted backlog.
It does not wait for the source to finish naturally. The SDK completes this
step before its future permits native server shutdown; `/health` and TCP
admission can remain available during SDK drain. The duration bounds SDK waiting,
not later server-wide shutdown, Rust destructor preemption or client receipt.
Expired physical work stays charged through actual release. Immediate does not
roll back committed application writes. Optional outcomes distinguish `Drained`,
`CancellationRequested`, `Expired` and `InfrastructureFailure`; late cleanup
does not rewrite a prior failure or expiry.

## Three resolver roles

[`src/resolvers.rs`](src/resolvers.rs) implements every generated trait:

1. `subscribe_readings` creates an owned `SubscriptionSource`. Its interval,
   counters and immutable backing outlive the borrowed setup request.
2. `resolve_readings_event` maps each owned item to the root `Reading`. Cloning
   this small value clones an `Arc<str>` rather than the complete message.
3. `ReadingResolver` resolves only selected `number` and `message` fields. The
   selected message materializes its ordinary typed String for serialization;
   there is no domain-to-JSON round trip or generated output DTO.

[`src/domain.rs`](src/domain.rs) bounds source state independently of GraphQL.
There is no producer task or upstream queue. Missed application clock ticks
have not become published events and do not change SDK delivery policy. All
application field/source errors use one `AppError` and the mapper registered
once at `Resolvers::builder`. Fresh event context preparation, authorization
and delivery rules remain the SDK contract. Replay, catch-up and watch-race
coordination belong to an application and are outside this minimum connection.
See [whole-configuration sizing](../../docs/subscriptions-walkthrough.md#whole-configuration-sizing)
before changing event or queue limits.

## Generated contract and workspace

```text
lab01/
  Cargo.toml                         # consumer member; generated root excluded
  chigiri-subscriptions.toml          # producer/schema/runtime/consumer inputs
  examples/subscriptions/Cargo.toml   # ordinary handwritten application
  generated/subscriptions-example/
    Cargo.toml                       # generated standalone [workspace]
    chigiri-generated.json            # ABI, producer, provenance and inventory
```

The parent workspace lists `examples/subscriptions` in `members` and
`generated/subscriptions-example` in `exclude`. A separate consumer workspace
uses the same arrangement, for example `members = ["app"]` and
`exclude = ["contract"]`. Keep the generated contract separate from handwritten
code, Cargo.lock and build caches. Both the generated crate and consumer must
use the same Chigiri source: here the producer's `[runtime] root = "."` and
the workspace path dependency resolve to this checkout.

Build and inspect the producer, then check protected output before compiling:

```console
make debug TOOLCHAIN=stable
export CHIGIRI="$PWD/target/$(rustc +stable -vV | sed -n 's/^host: //p')/debug/chigiri"
"$CHIGIRI" rust --version --format json
"$CHIGIRI" rust check --manifest chigiri-subscriptions.toml --format json
cargo +1.85.0 test --locked --offline -p chigiri_subscriptions_example --all-targets -- --test-threads=1
```

The generated marker records ABI 8 and producer revision
`2b788d39e7dce391c9bcabcb78f35a5c22c30354-dirty`, Rust 1.95.0 and the local
debug build. This is an implementation worktree identity, not a clean release.
`generated::generator()` exposes its producer information. Match SDK source,
ABI and producer build identity when adopting another checkout; compilation
alone does not prove generator freshness. Use protected `rust update` after
deliberate SDL/manifest changes, followed by `rust check` and consumer compilation.
Never edit generated files or ownership hashes manually. The repository's
registered inventory includes this manifest and verifies the generated tree.

Generated package license and runtime dependency-version metadata are a separate
packaging candidate, Beads `lab01-qke`; this example does not change generator
formats or treat that candidate as a subscription runtime failure.

## Checked behavior

The native test uses the real Router and loopback listener for Query data,
POST-SSE data/metadata, wrong route/Origin rejection before setup, disconnect
cancellation, no-hook owner release, default/overridden Graceful, Immediate,
expiry, SDK-first health/admission behavior and safe initial/committed-body
failure. Test-owned context gates exercise slow or failing application work
without adding fault flags to the executable. Every native task is joined on
successful and failed paths. Checks are local macOS/MSRV/stable evidence, not
production capacity, browser/proxy, remote platform or delivery guarantees.
