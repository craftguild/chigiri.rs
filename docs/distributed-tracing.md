# Distributed tracing in Rust applications

Register execution instrumentation once on the Endpoint's existing component
graph. Context factories, SDK-invoked providers, plugins and generated resolvers
then use ordinary `opentelemetry::Context::current()` during their actual work.
No context span field, request plugin, generated edit or resolver attribute is
required. The default facade and core do not depend on OTel or tracing.

This guide describes the locally implemented ADR-0056 contract. The
validation evidence identifies executed
checks and limits; source-bound independent reviews are recorded in Beads
`lab01-ija.9` and `lab01-ija.10`. External platforms, collectors and production
capacity qualification remain outside this local acceptance.

## One registration

Create the tracer using your application-owned provider and processors, then
inject it while building the Endpoint:

```rust,ignore
use chigiri_opentelemetry::OpenTelemetryInstrumentation;

let endpoint = chigiri::Endpoint::builder(resolvers)
    .inject(|components| {
        components.observability()
            .instrumentation(OpenTelemetryInstrumentation::new(tracer.clone()));
    })
    .context_factory(prepare_context)
    .build()?;
```

Use `.without_context()` for C=(). Use `.field_instrumentation(true)` on the
same observability component only when actual per-field scopes are wanted.
The disabled path installs no per-field instrumentation plugin or wrapper.
Repeated `instrumentation` calls replace that slot in call order; independent
lifecycle and error observer slots keep their own registrations.

The maintained [registration consumer](../crates/testkit/tests/usability/instrumentation_registration.rs),
[native factory/resolver consumer](../crates/testkit/tests/usability/distributed_tracing/native.rs)
and [subscription consumer](../crates/testkit/tests/usability/distributed_tracing/subscription_events.rs)
compile this pattern against existing generated ABI 8 crates. They use C=()
and the existing generated resolver/error-mapper interfaces.

The application owns provider/exporter/subscriber initialization, its ordinary
server/runtime and their shutdown. Endpoint shutdown ends SDK work and retains
its existing physical cleanup guarantees; it does not flush or shut down the
application's telemetry provider. Keep processors nonblocking and apply your
own bounded exporter policy. Exporter storage is not promised by observer
delivery, a span's end or a successful GraphQL result.

## Incoming parent and local control

Before SDK handoff, Chigiri selects a valid current OTel SpanContext. This
includes one activated by HTTP middleware. Otherwise it extracts bounded W3C
`traceparent` and `tracestate` using the released propagator. Otherwise recording
starts a new root. A conflicting opaque Chigiri `TraceId` is never a distributed
parent; effective telemetry correlation follows the actual SDK context. Direct
execution has no implicit HTTP carrier.

The standard telemetry-suppression boolean is captured separately before parent
selection, including when no valid span exists. Header fallback, fresh events,
worker movement and optional-correlation pressure preserve that boolean. Every
SDK activation installs its explicit parent/control and restores the worker's
previous values on poll return, destruction or unwind. SDK retention excludes
baggage, arbitrary Context values and application extensions.

The three controls remain distinct:

| Control | Effect |
| --- | --- |
| Standard OTel suppression | SDK spans, including diagnostic conversion, are not created; the flag remains visible to downstream/application instrumentation. Independent ErrorObserver delivery and GraphQL work continue. |
| Chigiri `SamplingDecision::Drop` | Ordinary SDK lifecycle spans are omitted while the adopted parent and suppression still propagate. Independent error delivery remains; diagnostic conversion requires an existing valid unsuppressed parent. |
| Application OTel sampler | Determines sampled/non-recording behavior for created spans. Chigiri does not force sampling or guarantee exporter storage. |

Invalid/duplicate/oversized tracing fields do not add GraphQL errors or bypass
HTTP admission. Reads are bounded before copying: traceparent at 512 bytes,
combined tracestate at 512 bytes/32 members. Optional trace retention degrades
before otherwise admissible subscription work is rejected. Suppression is
fixed control and survives even when optional parent/correlation is discarded.
The provider caps its own simultaneous owned SDK spans at 4096; capacity or
provider decline preserves propagation/control without opening a substitute
diagnostic root. That bound does not claim a process-wide application heap or
exporter limit.

## Outgoing clients and ordinary tracing spans

Chigiri does not intercept application network calls. Use your client's normal
OTel integration or inject the current context into its carrier explicitly:

```rust,ignore
use std::collections::HashMap;
use opentelemetry::{Context, propagation::TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;

let mut carrier = HashMap::<String, String>::new();
TraceContextPropagator::new().inject_context(&Context::current(), &mut carrier);
// Apply the carrier to the application-owned outgoing request.
```

Do not keep a thread-local Context guard across an await. Standard future
instrumentation activates/restores context for each poll. Outgoing tracing
instrumentation should also honor the standard suppression flag; the local
control is not a new HTTP suppression header.

For ordinary contextual `tracing` spans, configure a compatible application
subscriber with context activation enabled:

```rust,ignore
use tracing_subscriber::layer::SubscriberExt;

let subscriber = tracing_subscriber::Registry::default().with(
    tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_context_activation(true),
);
```

Install/use that subscriber through the application's normal tracing setup.
Ordinary `tracing::info_span!("application.resolver")` with
`tracing::Instrument` then joins current SDK execution, including beneath an
outer middleware span. `tracing::Span::current()` need not itself be an
SDK-created span. The maintained [interop consumer](../crates/testkit/tests/usability/distributed_tracing/interop.rs)
checks real parent IDs, incoming sampling flags and a small loopback downstream
carrier, using OTel API 0.32.0, SDK 0.32.1, tracing-opentelemetry 0.33.0,
tracing 0.1.44 and tracing-subscriber 0.3.22. These are opt-in backend and
consumer dependencies; they do not upgrade the SDK's minimum Rust 1.95.

## Application-created tasks

SDK-owned transitions propagate automatically. Capture context explicitly for
tasks spawned by application code, using the standard ecosystem API:

```rust,ignore
use opentelemetry::{Context, context::FutureExt};

let context = Context::current();
let task = tokio::spawn(async move {
    // Application-owned background work.
}.with_context(context));
```

Also propagate the application subscriber when required by its tracing setup.
This explicitly captured complete Context belongs to the application; choose
its values and lifetime deliberately. Chigiri does not discover detached tasks,
join them during endpoint shutdown or automatically retain arbitrary values
from them.

## Subscription traces

Setup adopts its upstream parent and releases its owned span when setup ends.
Source waiting retains only the already-budgeted setup SpanContext and explicit
suppression control. It does not keep setup C, a complete Context or a live
setup span.

Each admitted event prepares fresh context/authentication/cache/state. Recorded,
unsuppressed events start distinct root traces with a creation-time setup link
when optional setup correlation remains available. The event telemetry ID is
that event's actual trace ID. Event `Drop` still selects a fresh parentless
boundary, and suppressed events run without SDK spans. Event-local suppression
does not change later events' immutable setup baseline. Publisher, connection
worker and previous-event context never become the next event's parent.

This policy is shared by WebSocket, POST SSE and multipart through both official
adapters. It adds no per-message trace payload, SDL field or delivery receipt.
Idle waiting, heartbeat traffic and unstarted backlog do not open event spans.
Existing queues, deadlines, dropped-result metadata, physical ownership and
shutdown outcomes remain authoritative. See the
[subscription walkthrough](subscriptions-walkthrough.md) for source/event roles
and application responsibilities.

## Migrating event-only observers

Replace built-in `OpenTelemetryObserver` lifecycle/error registrations with the
single `OpenTelemetryInstrumentation` registration for execution propagation.
Keep unrelated custom lifecycle/ErrorObserver registrations when needed. The
SDK does not automatically register the legacy observer alongside the provider.

`OpenTelemetryObserver` and `TracingObserver` constructors remain available for
event-only use. Their events/internal relationships do not establish automatic
incoming parent adoption or context during resolver work. Deliberately
registering the OTel legacy observer and execution provider together records
overlapping SDK lifecycle spans. Remove the redundant built-in path when
migrating rather than treating duplicate spans as separate executions.

No generated crate update, ABI migration, Mock default tracing or Luau API is
part of this registration change. Preserve the application's existing error
allowlist, audit observers and exporter policy.
