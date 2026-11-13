# ADR-0056: Distributed tracing context propagation through SDK execution

Status: Accepted / Implemented with focused local macOS acceptance

Date: 2026-10-24

Related: ADR-0004, ADR-0012, ADR-0016, ADR-0017, ADR-0053, ADR-0054, and ADR-0055.

Tracking: Beads `lab01-oud` covers design publication only. Implementation and
reviews require separately scoped issues under the
[review workflow](../review-workflow.md). Publishing this ADR does not implement
its APIs or establish acceptance evidence.

Local implementation: Beads `lab01-ija`. The
[integration guide](../distributed-tracing.md) and
focused validation evidence describe the
current implementation. Source-bound independent acceptance is recorded in
Beads `lab01-ija.9` and `lab01-ija.10`; their scope and the validation record
preserve local limits and the separate pre-existing baseline follow-up.

A subsequent designer review in Beads `lab01-gga` accepted the focused local
implementation after independently verifying the legacy observer cleanup
correction in `lab01-gga.1`. Both are closed; their decisions apply to the
recorded source snapshots and do not establish full-product or non-local
qualification. Suppression prevents new spans and fallback telemetry, but does
not defer ending or releasing already-owned request and phase spans.

2026-10-24 clarification: Beads `lab01-f5e.1` identified that SpanContext alone
does not retain OpenTelemetry's standard telemetry-suppression control. Sections
4 through 6 and the acceptance rows explicitly preserve that bounded control
without admitting baggage or arbitrary Context values. These requirements are
normative for implementation and independent acceptance.

Implementation goal: Prompt for a new agent.

## 1. Context and decision

The existing `chigiri_opentelemetry::OpenTelemetryObserver` converts lifecycle
and error events into spans using an application-owned tracer. Its request
spans start from an empty OpenTelemetry context. The observer's private span
map preserves internal relationships, but neither adopts an incoming parent
nor activates its execution span while application code runs. An opaque
`RequestTelemetry::trace_id()` alone cannot supply a parent span, trace flags,
or trace state. The existing `TracingObserver` reports events rather than
instrumenting the complete asynchronous execution scope.

Applications can compensate with context fields and execution plugins, but
that does not cover earlier transport phases and requires knowledge of SDK
task boundaries. ADR-0055 makes task ownership an SDK responsibility; moving
work into those tasks must not disconnect an otherwise correctly configured
application trace.

Add opt-in SDK instrumentation that adopts an upstream parent, propagates
execution context through SDK-owned work, and exposes the current context to
ordinary application code. Users configure it once rather than adding a span
to every `AppContext`, instrumenting every resolver, editing generated code,
or installing a special request plugin. Standard OpenTelemetry APIs remain the
application interface, including interoperability with ordinary `tracing` spans.

## 2. Amended and preserved boundaries

This ADR amends ADR-0012 Sections 2.1, 2.6, and 3.1 only as follows: the opt-in
backend may interpret standard propagation headers and create provider-owned
trace identities, and the SDK may carry backend-neutral execution scopes.
Correlation metadata is no longer the only supported tracing boundary.
`TraceId`, request ID, error ID, and operation identity remain distinct.
Never reinterpret an arbitrary opaque `TraceId` as a distributed parent.

Keep OpenTelemetry types, `tracing::Span`, exporters, and semantic conventions
out of generated contracts and the public core/facade signatures. The core
remains standard-library-only; default consumers acquire no OpenTelemetry or
`tracing` dependencies. Backend-specific construction and conversion stay in
the opt-in `chigiri_opentelemetry` crate. A backend-neutral instrumentation SPI
belongs at the shared runtime/HTTP boundary, with executor scope hooks where
needed; it is read-only with respect to GraphQL execution.

The SDK owns context propagation and span lifetime for its work. Adapters only
forward bounded transport information and invoke the shared capture boundary
before handing work to SDK tasks. They do not implement separate tracing
policy for Axum, Hyper, or individual subscription protocols. Preserve
ADR-0055's lifecycle owner, deadlines, physical accounting, and cancellation.

The application owns tracer providers, exporters, subscribers, their runtime
requirements and shutdown, and downstream HTTP/RPC clients. Chigiri does not
install a global provider/subscriber/propagator, start an exporter, open a
listener, or flush application telemetry during endpoint shutdown.

## 3. One registration and ordinary resolver code

Add `ObservabilityComponents::instrumentation(provider)` to the existing
`inject` namespace. It replaces one complete optional instrumentation slot in
call order under ADR-0017. There is no second endpoint builder or plugin list.
Leave existing lifecycle/error observer slots independently configurable;
registration must not silently replace them.

Provide `chigiri_opentelemetry::OpenTelemetryInstrumentation::new(tracer)` as
the official implementation. It includes standard W3C Trace Context extraction
without requiring separate lifecycle/error observer registration. The following
is an illustrative registration fragment using application-owned builder/tracer
values. Maintained public consumers exercise the same registration API:

```rust,ignore
let instrumentation =
    chigiri_opentelemetry::OpenTelemetryInstrumentation::new(tracer.clone());

let builder = builder.inject(|components| {
    components.observability().instrumentation(instrumentation);
});
```

Within the context factory, plugins, resolvers, and SDK-invoked providers,
`opentelemetry::Context::current()` exposes the applicable execution context.
Application-created child spans and outgoing propagation use standard APIs:

```rust,ignore
let current = opentelemetry::Context::current();
propagator.inject_context(&current, &mut outbound_carrier);
```

The application supplies the standard propagator and client carrier in that
example. Chigiri does not intercept arbitrary outgoing network calls. A
maintained consumer must show an actual downstream request and its parentage,
not just the presence of an opaque correlation string.

Also support applications using a compatible `tracing-opentelemetry` layer
with context activation enabled. Ordinary contextual `tracing` spans created
inside resolver code must join the SDK execution context, even when an outer
HTTP middleware has its own span. The current OpenTelemetry 0.32 dependency
has a compatible tracing bridge in the 0.33 release family. Verify the actual
dependency combination and MSRV in the consumer; do not select an unrelated
latest release. This interoperability does not require that
`tracing::Span::current()` itself be an SDK-created tracing span.

No application context field, generated helper, custom resolver trait,
per-resolver `instrument` attribute, or application plugin is required merely
to receive the SDK context. Applications may still create their own spans.

## 4. Incoming parent and bounded propagation data

Capture the parent at each supported SDK entry before its first task handoff
and before request-start observation or application preparation. Cover direct
execution, finite HTTP, and subscription setup through both official adapters.
Apply this precedence once:

1. Use the valid current OpenTelemetry span context, including one activated
   by the application's HTTP middleware and compatible tracing layer.
2. Otherwise extract a valid parent from HTTP `traceparent` and `tracestate`.
3. Otherwise start a new trace when SDK span creation is enabled.

Do not read ambient context later from an unrelated worker to reconstruct the
parent. An existing valid context wins even when headers contain another
valid parent. Direct entry has no implicit HTTP carrier. An opaque transport
`TraceId` remains correlation-only when no propagation context is available.

Use a compatible, unmodified released OpenTelemetry W3C propagator, not a
Chigiri-owned Trace Context parser. Its carrier adapter bounds reads before
copying: at most 512 bytes for traceparent and 512 combined bytes for
tracestate, with at most 32 state members. Duplicate traceparent is invalid;
combine tracestate according to the standard within these bounds. Preserve a
valid traceparent without tracestate when the latter is invalid or too large.
Missing, malformed, unsupported, or oversized propagation information falls
back to the applicable parent/new-root rule. It does not add a GraphQL error.
Existing HTTP header and request-admission limits still apply independently.

Carry only the selected span context (trace ID, span ID, flags, remote/local
identity, and bounded trace state) plus one explicit boolean for OpenTelemetry's
standard telemetry suppression. Capture that boolean from the current owning
execution context before parent selection, even when it has no valid span.
Selecting a header parent, starting a new root, or dropping optional tracing
correlation must not clear it. This is local instrumentation control, not a
W3C header, sampling flag, client setting, or new configurable limit.

Do not automatically retain arbitrary values from an OpenTelemetry `Context`,
incoming baggage, credentials, or application extensions. Tracing metadata is
untrusted and supplies no authentication, tenant identity, quota key, or
authorization decision. Do not copy the complete Context merely to retain its
suppression flag.

SDK-created context supplies the actual trace ID to telemetry correlation when
one exists; a conflicting legacy opaque ID cannot override it. Error/request
IDs retain their own creation and failure rules. Failure to create a request
ID must not break propagation through an already-owned execution scope.

## 5. Execution scopes, sampling, and cleanup

Use owned scopes rather than a global lookup keyed only by request ID. A
request scope captures its parent once, creates the SDK request span where
recording is enabled and telemetry is not suppressed, and owns child
operation/provider scopes. Each scope is
activated for synchronous application invocation and every future/stream poll
under that scope, including after SDK task handoff. Restore the previous
context before returning Pending/Ready or unwinding. Apply the appropriate
scope during SDK-owned cancellation and future destruction as well.

Install the owned suppression value along with the selected span context on
every activation, using the standard OpenTelemetry suppression APIs in the
backend. Preserve both suppressed and unsuppressed cases, independently of
parent validity. Do not infer suppression from an unrelated worker's ambient
context. A child SDK handoff captures the current control inside its owning
execution, including a nested suppression scope, rather than blindly restoring
the request's initial unsuppressed value. Poll return, unwind and destruction
restore the previous ambient suppression as well as the previous parent;
neither value may leak into another request or endpoint.

Never hold a thread-local context guard across an arbitrary await. Preserve
explicit parentage when sibling fields interleave, tasks move between threads,
or multiple endpoints execute concurrently. Per-field scope creation remains
opt-in through `field_instrumentation(true)`. Disabling it must not introduce
per-field wrappers, lookups, or spans merely to keep request/operation context.
When enabled, the field span surrounds actual resolver work rather than only
the synchronous observer callback.

The guarantee covers SDK-owned task transitions. Application-created detached
tasks use standard explicit context capture/future instrumentation. Document
that boundary with a small example; do not promise automatic propagation to
arbitrary threads, third-party executors, or user-created tasks.

Keep telemetry suppression and the two sampling controls distinct:

- Standard OpenTelemetry suppression prevents SDK-owned OTel/tracing span
  creation, including error-event conversion into backend spans, while keeping
  that suppression active for application code and downstream instrumentation.
  It is not Chigiri `Drop` or a provider sampling decision. Do not generate
  replacement roots or diagnostic spans to bypass it, including on parent
  fallback, scope cleanup or capacity degradation.
- Chigiri's existing decision is fixed once per request/event and controls its
  lifecycle recording. `Drop` suppresses SDK lifecycle spans but must still
  propagate a valid adopted parent. With no parent and lifecycle recording
  dropped, the current context may have no valid span; preserve suppression
  regardless. Application instrumentation can start its own trace when not
  suppressed. Do not invent sampled SDK roots to hide this case.
- The application-owned OpenTelemetry sampler controls whether created spans
  record/export and how parent sampling flags are honored. Chigiri `Record`
  never forces an upstream unsampled trace to become sampled. A valid
  non-recording span context remains usable for propagation.
- Internal error events remain delivered to the error observer independently
  of lifecycle sampling or telemetry suppression. Preserve ordinary lifecycle
  observer delivery, GraphQL execution/results and security-audit policy as
  well. Suppression controls backend instrumentation rather than those
  independent notifications. Observer delivery is not an unconditional
  exporter guarantee; do not create an unrelated sampled root to bypass the
  OTel sampler or suppression control.

Successful, rejected, failed, timed-out, cancelled, and unwound work must end
each owned span at most once and release scope references. Preserve existing
terminal-event ordering, masking, observer panic policy, and physical cleanup.
Instrumentation completion describes SDK work, not client receipt or exporter
flush. No lock may be held while invoking application observers/tracers.

Recording/export failure or exhausted instrumentation capacity cannot change
a GraphQL result, admission decision, or shutdown outcome. Degrade telemetry
within its bounds; do not accumulate an unbounded fallback registry. Telemetry
callbacks and the configured span processor must not block on exporter I/O or
queue capacity. Ordinary telemetry remains separate from security audit policy.

## 6. Subscription event topology

Setup adopts the incoming parent using Section 4 and ends its execution span
when setup work ends; do not hold it open for the entire subscription. Retain
only its bounded SpanContext and separately captured suppression boolean as
immutable propagation metadata, including when the SpanContext is invalid.

Every admitted event gets a fresh execution scope. When lifecycle recording is
enabled and telemetry is not suppressed, its root starts a new trace and
includes one span link to the valid setup SpanContext at creation.
Event context construction, authorization,
resolvers, and response preparation run under that event's context. Its
telemetry trace ID is the event's actual trace ID, not a copied setup ID.
Do not inherit ambient publisher, connection-worker, or previous-event context.
Under Chigiri `Drop`, preserve the fresh event boundary without creating a
recorded root or reverting to setup as its parent.

Each event installs the setup's captured suppression before context preparation,
polling and destruction. Creating a fresh event root must not re-enable
instrumentation after suppressed setup. When suppressed, create no SDK event
span but still execute the event under a suppressed context, even without a
parent or setup link. Publisher/worker suppression never substitutes for this
subscriber-owned value. An event-local nested suppression scope follows only
that event's child work; it does not change the immutable setup baseline for
later events.

Source waiting does not create an indefinitely open event span. Poll sources
under an explicitly selected subscriber context, never publisher ambient
context; create the event execution scope when an item is admitted for event
work. Do not invent event spans for heartbeats, queued-but-unstarted work, or
events discarded before execution. Span completion does not claim delivery
through a network buffer or consumption by the subscriber.

This topology is shared by WebSocket, POST SSE, and multipart. WebSocket
operations use the captured opening parent for setup; do not introduce a new
per-message propagation payload or GraphQL SDL field. An application that needs
publisher-to-subscriber causal links is a separate future extension.

Retained trace state is included in existing metadata/accounting boundaries
before it is copied or retained. Exhausted tracing retention capacity drops
optional tracing correlation instead of rejecting otherwise admissible work.
The fixed suppression boolean remains present when optional correlation is
dropped; telemetry pressure must not disable the recursion guard.
Keep the existing subscription counts, queue policy, OR-based limits, physical
owners, and fresh application context/authentication/cache per event. Never
retain setup `C`, a live setup span, or the complete setup OpenTelemetry context
as an event context. Endpoint/runtime shutdown must not leave a telemetry task
or span map keeping those resources alive.

## 7. Compatibility and alternatives

The new provider is additive and opt-in. Keep existing observer constructors
and event-only registration working, but document that observers alone do not
provide the new propagation guarantee. A migration replaces the old built-in
OTel lifecycle/error observer registrations with the single instrumentation
registration; unrelated custom observers remain supported. Do not record the
same SDK span once from execution scopes and again from an automatically
installed legacy observer. Document duplicate recording if an application
deliberately installs both built-in paths.

Preserve generated ABI 8, resolver/error-mapper signatures, lifecycle/error
event contracts, and all Mock/CLI, reference, log, snapshot, and wire formats.
This ADR does not enable OTel in Mock by default or add a Luau API, metrics/logs
project, exporter manager, server wrapper, tracing DSL, or new runtime.

Rejected alternatives are requiring application context/plugin plumbing for
ordinary use; merely replacing the observer's empty context with a late
`Context::current()` lookup; exposing backend types through generated code;
keeping one long trace for all subscription events; and duplicating tracing
policy in each adapter. These either leave task-boundary gaps, increase
coupling/boilerplate, or retain an unnecessary long-lived trace.

## 8. Focused acceptance

The following are implementation obligations, not passing results.

| ID | Required behavior and focused evidence |
| --- | --- |
| TR-01 | A public-only generated consumer configures one instrumentation provider; context factory and resolver see the current context without a context span field, plugin, or generated edit. |
| TR-02 | Known upstream trace/span IDs reach direct and real HTTP execution with correct SDK child parentage; current middleware context wins over conflicting headers and opaque IDs. |
| TR-03 | SDK handoff, thread movement, Pending/resume, concurrent requests/endpoints and sibling fields preserve parentage and standard suppression, including nested owning scopes, and restore both ambient values without leakage; request-ID failure does not disconnect scopes. |
| TR-04 | An actual downstream carrier carries the current trace and span relationship; a compatible tracing layer's ordinary contextual resolver span is a child of SDK execution, including with an outer middleware span. |
| TR-05 | Each recorded subscription event has a distinct root trace and creation-time setup link; setup closes promptly, event telemetry matches its root, and fresh auth/context persists across both adapters and all profiles. Captured setup suppression survives fresh event scopes with or without a valid parent; publisher/previous-event state is absent. |
| TR-06 | Standard suppressed/unsuppressed, valid/no-parent, Chigiri Record/Drop and OTel sampled/non-recording cases preserve propagation, independent observer delivery and event isolation without forced instrumentation. Suppressed work creates no SDK backend spans, while application code still sees the suppression flag. |
| TR-07 | Success, rejection, timeout, disconnect, cancellation, unwind and SDK shutdown end scopes once, preserve owned suppression during disposal, restore parent/suppression and release references without weakening physical ownership or masking. |
| TR-08 | Invalid/duplicate/oversized carriers, baggage, capacity exhaustion and unavailable/full export paths degrade telemetry only; parent fallback or discarded optional correlation never clears suppression. Retained metadata is bounded and secret values remain absent from default spans, diagnostics and responses. |
| TR-09 | Opt-out consumers remain free of tracing/OTel dependencies; field instrumentation disabled adds no per-field work, enabled scopes cover actual work, legacy observers and custom observer slots remain supported without automatic duplicate spans. |
| TR-10 | A maintained upstream-to-resolver-to-downstream example and focused MSRV/stable checks identify exact source, dependency versions and executed scope; no external collector or broad qualification is required. |

Use an in-memory recording provider capable of distinct trace IDs, parent IDs,
flags, links and end counts. A fake tracer assigning one trace ID to every
request cannot prove propagation or isolation. Include standard-API suppression
assertions with valid and absent parents, header fallback, task/poll/drop
boundaries, fresh events and restoration to both ambient suppression states.
Spy backend creation counts and independent observer calls as well as the
current flag: sampling tests alone cannot establish suppression. Keep all
assertions independent of Beads, approval prose, previous validation manifests,
and sibling projects.

Run bounded local macOS tests for affected runtime/HTTP/adapter/observability
paths, consumer compilation, dependency boundaries, formatting, Clippy,
rustdoc, implementation rules, and whitespace. Use small loopback downstreams;
no external collector, remote Linux, cross/distribution build, browser, load,
soak, fuzz, or full-product validator is an acceptance prerequisite. Existing
support tiers and separate release qualification remain unchanged. Record
implementation and independent review evidence in Beads without a new review
sheet. The original design publication did not establish implementation or
runtime evidence; the linked local validation record now supplies the executed
scope for the implemented contract.

## 9. References

- [W3C Trace Context](https://www.w3.org/TR/trace-context/) defines propagation headers and their processing model.
- [OpenTelemetry Context](https://docs.rs/opentelemetry/0.32.0/opentelemetry/context/struct.Context.html) and [FutureExt](https://docs.rs/opentelemetry/0.32.0/opentelemetry/context/trait.FutureExt.html) describe activation and per-poll context propagation.
- [OpenTelemetry telemetry suppression](https://docs.rs/opentelemetry/0.32.0/opentelemetry/context/struct.Context.html#method.enter_telemetry_suppressed_scope) is standard Context control for avoiding recursive instrumentation; it is separate from SpanContext and sampling.
- [OpenTelemetry tracing API](https://opentelemetry.io/docs/specs/otel/trace/api/) defines parent contexts, non-recording spans, links, and span completion.
- [tracing-opentelemetry 0.33 source](https://docs.rs/tracing-opentelemetry/0.33.0/src/tracing_opentelemetry/layer.rs.html) documents context activation and contextual parent selection for the compatible integration.
