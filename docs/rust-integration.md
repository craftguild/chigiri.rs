# Rust resolver integration

Chigiri generates one complete resolver trait per root and object. Implement
ordinary Rust methods, bind domain values through `SchemaTypes`, and register
one application-error mapper when assembling groups. The generated crate never
depends on the application or domain crate.

For an executable separate-domain example, see the
[domain values and errors](../crates/codegen/tests/fixtures/trait-errors/domain.rs),
[ordinary resolver methods](../crates/codegen/tests/fixtures/trait-errors/consumer.rs),
and [direct execution assertions](../crates/codegen/tests/fixtures/trait-errors/direct.rs).
The [books example](../examples/books/README.md) supplies an application-owned server.

## Generated package license

Choose the generated package's license in the existing `[package]` table of
`chigiri.toml`, for example `license = "MIT OR Apache-2.0"` when that is your
chosen license. This optional Cargo metadata is separate from Chigiri's own
license and the runtime dependency version. Omission emits no `license` field;
Chigiri does not infer one or copy license files. Empty, whitespace-only,
control-containing and non-string values are rejected.

Run `chigiri rust update` to adopt the metadata and `chigiri rust check` to
verify exact output. Generated `Cargo.toml` remains owned; edit the source
configuration rather than generated files. Existing config-v2 manifests and
ABI 8 contracts need no migration when the setting is omitted.

## Git runtime version pins

When a dependency policy requires an explicit package version even for pinned
Git sources, add the actual runtime package version to `chigiri.toml`:

```toml
[runtime]
source = "git"
url = "https://example.org/team/chigiri.git"
revision = "<actual complete lowercase 40-character commit>"
version = "0.1.0"
```

Chigiri emits `version = "=0.1.0"` alongside the configured `git` and `rev` in
the generated Cargo dependency. Cargo verifies the package at that commit;
there is no registry fallback. Use a bare SemVer in the configuration, not
`=0.1.0`, `^0.1.0`, or a range. The optional version is independent of the
generated crate and generator versions; omission retains source-only selection.
Run `chigiri rust update` to adopt the setting, then `chigiri rust check` to
verify exact output. Generated Cargo manifests remain generator-owned.

## Field implementations and errors

`SchemaTypes::User = domain::User` retains the domain value. A selected field
method receives its borrowed parent and returns its exact Rust output through
`Result<Output, AppError>`. Stored, calculated, fallible, and asynchronous fields
use the same method form. Methods may appear in any order in an `impl` block;
every generated field is required. Roots that also occur as nested objects
retain the existing optional-parent signature for Query/Mutation. A nested
subscription root has its ordinary borrowed-parent methods separately from
source initialization and event resolution, as described below.

Group registration remains complete and schema-ordered:

```rust,ignore
let resolvers = generated::Resolvers::<AppContext, Types>::builder(map_app_error)
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_user(UserFields)
    .build();
```

All groups use one `AppError: Error + Send + Sync + 'static`. The mapper is
synchronous `Fn(AppError) -> FieldError + Send + Sync + 'static`. It runs once
for each selected resolver's returned error after that resolver is awaited.
Successful and unselected fields do not invoke it. The completed public type
is still `Resolvers<AppContext, Types>`.

Use an explicit public allowlist:

```rust,ignore
fn map_app_error(error: domain::AppError) -> chigiri::FieldError {
    match error {
        domain::AppError::NotFound => {
            chigiri::FieldError::public("User was not found").code("NOT_FOUND")
        }
        other => chigiri::FieldError::internal(other),
    }
}
```

`Display`, `Debug`, and serialized error contents are never exposed
automatically. Internal sources remain available to observers, while GraphQL
responses retain normal masking, error IDs, aliases, locations, paths, partial
data, and null bubbling. Conversion, middleware, and completion failures already
use `FieldError` and bypass this application mapper, including list-item errors.
List outputs keep ordinary `Vec<T>`/`Vec<Option<T>>` shapes.

An API-owned function can map an external domain error without an orphan-rule
implementation or a Chigiri dependency in the domain. Ordinary `?` uses Rust's
normal `From` conversions. Applications already using `FieldError` register
`std::convert::identity::<chigiri::FieldError>`; infallible applications can
register `|error: std::convert::Infallible| match error {}`.

## Per-request context

Use the existing `context_factory` and `PreparedContext::execution_only` path.
A named function expresses the request lifetime directly:

```rust,ignore
fn prepare_context(
    request: chigiri::HttpRequestContext<'_>,
) -> chigiri::BoxFuture<'_, Result<
    chigiri::PreparedContext<AppContext, ()>,
    chigiri::PrepareError,
>> {
    Box::pin(async move {
        let context = AppContext::prepare(request).await?;
        Ok(chigiri::PreparedContext::execution_only(context))
    })
}
```

`AppContext::prepare` belongs to the application. It may authenticate the request
and allocate request-local caches and sessions. Return `PrepareError::Reject`
for explicit transport rejection and `PrepareError::Internal` for masked
preparation failures. These failures do not enter the field-error mapper.

A closure can capture shared services through `Arc`. Give the returned closure
the explicit higher-ranked boxed-future signature so Rust preserves the request
lifetime through `ContextFactory`:

```rust,ignore
fn with_services(
    users: std::sync::Arc<domain::Users>,
) -> impl for<'a> Fn(chigiri::HttpRequestContext<'a>) -> chigiri::BoxFuture<
    'a,
    Result<chigiri::PreparedContext<AppContext, ()>, chigiri::PrepareError>,
> + Send + Sync + 'static {
    move |request| {
        let users = std::sync::Arc::clone(&users);
        Box::pin(async move {
            let context = AppContext::with_users(request, users).await?;
            Ok(chigiri::PreparedContext::execution_only(context))
        })
    }
}

let endpoint = chigiri::Endpoint::builder(resolvers)
    .context_factory(with_services(users))
    .build()?;
```

Construct a fresh context on every call. Share service/pool handles deliberately;
keep request identity, caches, and session state in the newly owned context.
Use `PreparedContext<C, S>` and `response_hook` when the response needs separate
state. Existing preparation ordering, deadlines, cooperative cancellation, and
future drop behavior remain authoritative. The application owns listeners,
routing, connection policy, and shutdown.

## Execution tracing

Use one `observability().instrumentation(OpenTelemetryInstrumentation::new(tracer))`
registration through `inject`. Ordinary context factories and generated
resolvers access `opentelemetry::Context::current()`; no span field, generated
edit or resolver attribute is needed. Per-field scopes remain opt-in.

The [distributed tracing guide](distributed-tracing.md) covers current/header
parent selection, standard suppression versus sampling, outgoing carriers,
ordinary contextual tracing, application-created tasks and migration from
legacy event-only observers. The application owns its provider, exporter,
subscriber and their shutdown. The SDK propagates through its own execution
and preserves existing endpoint lifecycle ownership.

## Typed subscription sources and application-owned streaming

ABI 8 adds complete subscription resolver groups to the same assembly. Each
field has an associated `Event: Send + Sync + 'static`, one
`subscribe_<field>` initializer and one `resolve_<field>_event` method. The
initializer may borrow context/arguments while awaited, but returns an owned
`SubscriptionSource<Event, AppError>` over the ecosystem `futures_core::Stream`
contract. The source itself is `Send + 'static`; it must not retain borrowed
setup state. The ordinary `<field>` method still resolves a single value when
the subscription-root object appears in a nested selection. It is not an alias
for source initialization.

Use the [complete consumer and executable walkthrough](subscriptions-walkthrough.md)
instead of implementing a second resolver model or copying partial signatures.
Selected fields retain ordinary typed output, aliases, null bubbling and error
mapping. The setup/source/field application errors use the one mapper on their
intended path; preparation and framework failures remain independently masked.

Context preparation runs once for setup and freshly for each retained event.
Return fresh principal, cache, state and budget values; share pools/services
deliberately. Chigiri retains bounded original subscriber metadata, not setup
context or publisher credentials. Revalidate changing permissions and the root
arguments on each event. Revoked credentials can reject the next preparation;
different credentials require resubscription. Initial response/header hooks do
not rerun as per-event header mutations.

The official bridges support `graphql-transport-ws`, distinct-connection SSE
and Apollo multipart subscriptions on an application-owned listener. The
SDK automatically owns managed execution, supervision, timers, cancellation and
bounded recovery. Construct the official service fallibly in the existing
Tokio runtime and register `chigiri_axum::service(graphql.clone())?` with ordinary
`Router::route_service`; Hyper provides `chigiri_hyper::managed_service`.
The application retains routes, runtime, TLS, HTTP version, connection admission,
signals and native server ownership. The default facade/core remains Tokio-free.
The [small Axum Query/POST-SSE application](../examples/subscriptions/README.md)
shows complete source/event/selected-field traits and cheap shared domain
backing without manual GraphQL drivers or final process-drain calls.

An optional `graphql.shutdown_on(signal, ShutdownOptions::graceful())` future
fits the native server's shutdown API. It closes/drains the SDK first, then
allows native stop; other routes and TCP admission can remain available during
that interval. Graceful defaults to 30 seconds, with a typed positive override;
Immediate requests cancellation without awaiting workers. No-hook registration
is supported but provides no coordinated drain notification. Native server
shutdown, arbitrary Rust destructor preemption and peer receipt are outside the
SDK wait bound. Optional `shutdown_outcome()` inspection adds no required await.

The Endpoint builder's `subscription_limits` and typed `inject` replace the same whole value;
all values are validated at `build`, not clamped at startup. Defaults include
128 active subscriptions, 32 queued events/256 KiB per subscription, 64 KiB per
encoded event and 64 MiB shared encoded buffers. Endpoint clones share quota
owners; independently built endpoints do not. See ADR-0054 for every bound and
hard maximum. The [checked whole-configuration guide](subscriptions-walkthrough.md#whole-configuration-sizing)
distinguishes raw/domain data, escaping, profile-inclusive encoded bytes,
candidate reservations and setup/control headroom, with default and 768-KiB
event/2-MiB queue examples. Configuration acceptance is not delivery or capacity
measurement.

Default `Backpressure` never silently drops a result. Only server-selected
per-source `SubscriptionOverflow::DropOldest` evicts whole unsent results; no
client argument selects overflow. Every event carries
`extensions.chigiri.delivery.droppedTotal`, null without dropping or otherwise
the current safe-integer cumulative count. Partial sends, controls and terminals
are not evicted. Proxy buffering and idle timeouts must be configured by the
application; heartbeat traffic does not extend event/send deadlines or certify
peer receipt. Upstream domain heaps, OS/proxy buffers, replay and acknowledgements
are outside these guarantees.

Logical cancellation/ID release is prompt, but blocked application work and
physical frame references remain charged through bounded cleanup/quarantine
until their real owners disappear. Keep the application-owned runtime alive
while draining; do not interpret a five-second cleanup cutoff as forced Rust
future destruction. The advanced Apollo Link recipe reads raw extensions
without requiring an SDL loss field or a Chigiri client SDK.

## Regeneration and editor workflow

Edit SDL, run protected `chigiri rust update`, then compile the consumer with
`rust doctor`. New fields and groups become compiler obligations. Use ordinary
trait implementation and navigation tools; generated SDL descriptions remain
on the declarations. Private adapters stay hidden from public rustdoc.

ABI 8 preserves ADR-0053's removal of projections, field setters,
`ResolverFuture` and `rust scaffold`, without aliases or another implementation
mode. See [migration](migration-0.1.0.md#abi-7-standalone-crates-to-abi-8)
for protected old output replacement and edit protection.

Compilation evidence is separate from editor evidence. IDE completion,
required-member insertion, and navigation remain unexecuted unless the
validation record names actual editor and
rust-analyzer versions and results.
