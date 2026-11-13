# ADR-0012: Standardize error identity and low-cardinality observability contracts

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-28
- Decision owners: Pythia maintainers
- Depends on: ADR-0009, ADR-0010, ADR-0011
- Revisits: ADR-0003, ADR-0006, ADR-0007

2026-10-24 amendment, locally implemented by ADR-0056:
[ADR-0056](0056-distributed-tracing-context-propagation.md) adds opt-in SDK
execution-context propagation and resolver access. Sections 2.1, 2.6, and 3.1
no longer restrict tracing to opaque
correlation metadata and event-derived spans: the optional backend adopts W3C
parents and the SDK activates backend-neutral owned scopes. Backend types stay
out of core/facade and generated signatures. Standard telemetry suppression is
explicitly carried alongside span context without retaining arbitrary Context
values. Sampling, redaction, event-only observers, and field-instrumentation
opt-in remain; subscription events use
separate linked traces. The local implementation and its focused evidence are
described in the [integration guide](../distributed-tracing.md). Source-bound
independent acceptance is recorded with ADR-0056; the remaining contracts of
this ADR are preserved.

## 1. Context

Pythia currently masks internal field errors behind a fixed message and code and
exposes the original source only to field plugins. Production systems need to
track transport, context-factory, persisted-store, rate-limit-provider,
execution, and response-encoding activity as one request, while safely
correlating client reports with internal logs.

Logging raw queries, variables, authorization data, field arguments, or error
sources by default would leak secrets or personal data. Using unbounded
operation names or field paths as metric labels would cause a cardinality
explosion. OpenTelemetry GraphQL semantic conventions are also still in
Development status, so Pythia cannot expose those external terms directly as a
semver-stable public event contract.

## 2. Decision

### 2.1 Separate error, request, trace, and operation identities

| Identity | Cardinality | Purpose |
| --- | --- | --- |
| request ID | per request | application/HTTP request correlation |
| trace/span ID | per tracing backend | distributed trace |
| error ID | per internal failure | correlation of a masked client error with its source |
| operation key | per exact document and operation | policy, metrics, and schema impact |

These identities are not interchangeable strings. An opaque Chigiri `TraceId`
is not interpreted as a distributed parent. Under ADR-0056, the optional backend
adopts or creates real span context and the SDK activates neutral owned scopes;
effective telemetry correlation uses that context's actual trace ID. Request
and error IDs retain their separate identity and failure rules.

### 2.2 Error ID

Each internal failure may receive an opaque `ErrorId`. Its public representation
is limited to 1–64 bytes of visible ASCII and rejects newlines, whitespace, and
header delimiters. Although the ID is not secret, it must not encode source
content, time, a principal, or a host path.

`ErrorIdFactory` is injected at the runtime/façade boundary; no random-number
dependency is added to `pythia_core`. A standard factory in an optional
observability crate can generate collision-resistant random IDs, while tests can
use a deterministic sequence factory.

Multiple representations of the same internal failure, such as those caused by
null bubbling, retain the same error ID. Independent sibling failures receive
different IDs. Each request has one request ID but may have several error IDs.

Only internal errors add the following fields to the client response:

```json
{
  "message": "Internal server error",
  "extensions": {
    "code": "INTERNAL_SERVER_ERROR",
    "errorId": "opaque-id"
  }
}
```

Masking remains active when no factory is configured, but `errorId` is omitted.
Production-readiness presets and gates require a configured factory.
`FieldError::public` cannot set the reserved `errorId` key.

### 2.3 Internal error observation

All phases report internal failures through a common `ErrorObserver`, rather
than limiting observation to field plugins.

The observer may inspect only:

- error ID, request ID, operation key, schema digest
- lifecycle phase
- stable code returned to the client
- a borrowed chain of internal source errors;
- response path and schema coordinate, when present
- provider/component's static name

The observer cannot return a source to the client or change the GraphQL result or
HTTP status. Observer panic is treated as process panic; there is no ordinary
observer-error return value. The framework chain never waits directly on an
asynchronous observer performing I/O. Instead, the application forwards events
to a bounded non-blocking sink.

The same diagnostic path covers not only resolver-returned internal errors, but
also value completion after resolution, list/output-node limits, and
introspection invariants, while retaining the `FieldError` source. The core
executor does not convert an error to `GraphQlError` until it has assigned the
error ID and notified the observer. Direct execution and HTTP endpoints
therefore preserve the same ID, source, and masking behavior.

### 2.4 Lifecycle events

Separate from interceptors, Pythia defines synchronous, read-only
`LifecycleObserver` events. The minimum event set is:

- endpoint/schema built
- request started/completed
- transport admission completed/rejected
- body/decode/source resolution completed
- document cache hit/miss
- validation/operation selection completed
- operation admission completed/rejected
- execution started/completed/cancelled
- internal error observed
- response encoded/failed

Each event carries a borrowed view of its phase, duration/result, and bounded
identity. Events cannot short-circuit or transform requests. Successful fields
do not emit events by default; only field errors and explicit instrumentation do.

### 2.5 Metrics

Pythia defines standard metric semantics without depending on a specific
backend.

- request count and in-flight count
- end-to-end/phase latency
- request/response bytes
- document cache, persisted store hit/miss
- validation/admission/rejection/error/timeout count
- operation cost distribution
- resolver/field error count
- rate-limit allow/reject/provider failure

Metric labels are restricted to this closed set:

- operation kind
- registered operation key or bounded hash
- shortened schema digest
- lifecycle phase
- stable result/error/rejection code
- bridge/provider's static name

Raw queries, operation variables, argument values, response paths, unregistered
client operation names, principals, tenants, IP addresses, request IDs, and
error IDs are prohibited as metric labels. An operation name is allowed only as
registered catalog metadata or a trace attribute, never as a metric-series key.

### 2.6 Tracing

Pythia's stable span model is as follows:

- request root span
- operation resolution/validation span
- operation execution span
- opt-in field/resolver span
- provider spans, such as persisted store and rate limit

Span names use bounded static phase names or operation kinds and never append a
client operation name. Query documents and variables are excluded from default
attributes.

OpenTelemetry conversion lives in the independent `pythia_opentelemetry` crate.
The conversion layer manages the Pythia event version and the adopted version of
the [OpenTelemetry GraphQL semantic conventions](https://opentelemetry.io/docs/specs/semconv/graphql/graphql-spans/),
so changes to external conventions do not change the core event ABI. `tracing`
integration may also live in a separate crate, but consumes the same stable
Pythia events.

ADR-0056 additionally supplies owned execution instrumentation through one
optional registration. Its contexts are active during SDK/application work,
including poll and destruction, with standard suppression separate from
sampling. Existing event-only observers remain independent compatibility paths.
Backend types stay outside neutral core/facade and generated signatures.

### 2.7 Redaction policy

The following are prohibited from default recording:

- raw GraphQL document
- variables, field arguments, result data
- authorization/cookie/header value
- Display/Debug error source full text
- source SDL text and host filesystem path
- principals, email addresses, wallets/addresses, and other product identities

An application may configure an explicit redactor or sampler to permit
additional attributes, but no switch enables all framework-reserved secret
fields. Pythia does not infer a development mode from environment variables to
enable raw-document logging.

`Debug` implementations follow the same rules; request and error contexts are
not blindly derived in full. Operation sources expose only document byte length
and whether a catalog hint exists. HTTP responses expose only status, header
names and value lengths, and body length—not raw documents, header values, or
response bodies.

### 2.8 Sampling and backpressure

- The sampling decision is fixed when a request starts. Additional error events
  may remain always-on.
- Observer callbacks must not perform blocking I/O.
- The standard async exporter bridge uses a fixed-capacity bounded MPSC and
  records drops caused by a full queue or disconnected consumer. Producer
  contention alone does not cause a drop.
- Telemetry producers use `try_send`; workers use `try_recv`. Neither waits for
  queue capacity or event arrival. Export failure never delays or fails a
  GraphQL request.
- Audit and safelist security sinks have failure policies separate from ordinary
  telemetry and follow ADR-0010.

The runtime surrounds the complete request chain directly at its execution
point; it does not mix an observer plugin into the user `RequestPlugin` list. A
field observer is added to the field chain only when
`field_instrumentation(true)` is enabled. When disabled, work proportional to
the number of fields—dynamic dispatch, future wrappers, and identity/telemetry
lookups—is absent. Terminal-event ordering remains identical for successful,
rejected, timed-out, and dropped request futures.

## 3. Public API boundary

The stable public API includes:

- `ErrorId`, `ErrorIdFactory`
- `RequestId`, `RequestIdFactory`, `RequestTelemetry`, `Sampler`, and the sampling
  decision made at request start
- a read-only `RequestIdentity` view
- closed lifecycle phase/result enums
- `LifecycleEvent<'a>` and `LifecycleObserver`
- `ErrorEvent<'a>` and `ErrorObserver`

Backend-specific meter, span, and log types, OpenTelemetry contexts, and
`tracing::Span` are excluded from the public APIs of `pythia_core` and `pythia`.
Events use a non-exhaustive strategy so future fields can be added compatibly;
semantic changes to phase or result enums require normal semver treatment.

### 3.1 Accepted Rust contract

The following types live in the std-only `pythia_core` crate and are re-exported
from `pythia`:

- `ErrorId` / `RequestId`: visible ASCII opaque newtype of 1 to 64 bytes. `Debug` does not display values.
- `ErrorIdFactory` / `RequestIdFactory`: synchronous, object-safe traits with
  explicit factory failures.
- `RequestTelemetry`: owned request/trace correlation and sampling snapshot,
  created exactly once at an HTTP or direct entry point.
- `Sampler`: a synchronous, object-safe trait that returns one decision from a
  provisional request identity at request start.
- `LifecyclePhase`/`LifecycleResult`: closed enum.
- `LifecycleEvent<'a>` / `ErrorEvent<'a>`: bounded identity and static component name borrowed read-only view.
- `LifecycleObserver` / `ErrorObserver`: Synchronous object-safe trait with no return value.

`pythia_observability` provides deterministic test and random factories, a
sampler, a bounded non-blocking exporter queue, and a closed metric-label
builder. `pythia_tracing` and `pythia_opentelemetry` are independent crates that
convert the same events without exposing backend types through this public API.
A full or disconnected queue drops the event through `try_send` and increments
only the drop counter. `TryExport` distinguishes `Exported`, `DroppedFull`, and
`Disconnected` as closed variants.

`RandomRequestIdFactory::new()` is called after a worker process forks and before
the server accepts requests. It obtains one 256-bit seed from the operating
system. Factory clones share the seed and an `AtomicU64` counter. Each request
hashes the `pythia-request-id-v1` domain separator and big-endian counter with a
BLAKE3 keyed hash and encodes the first 128 bits as 32 lowercase hexadecimal
characters. Request generation performs no per-request OS randomness, mutex
locking, or heap allocation for hash state. Counter exhaustion returns
`RequestIdFactoryError` rather than a duplicate ID. Factory `Debug` output never
includes the seed, counter, or generated ID. Failure to obtain a seed fails
factory construction before startup. `RandomErrorIdFactory` remains dedicated
to the error hot path and uses OS randomness for each error.

The automatic baseline creates no factory in `Endpoint::builder`; worker-local
final `build()` performs `new()` and the initial probe. A prefork server may
prepare the builder in its parent, but constructs the endpoint only after the
worker forks. The official factory and initial-probe wrapper retain their
creator process ID and fail closed in another process before touching the
counter or initial ID. An `AtomicBool` one-shot transfers the probe ID without a
per-request mutex, poisoning, or a lock held across a lower-level factory call.
Third-party factories accept concurrent calls under their `Send + Sync`
contract and are created inside workers for prefork use.

The legacy OTel observer retains bounded active-span state keyed by request ID
for event-only conversion. ADR-0056's separate execution provider owns scopes
directly, without an ID lookup, and propagates selected parent/suppression
through actual operation, provider and opt-in field work. Subscription events
use fresh linked roots. Chigiri `Drop` omits lifecycle spans while independent
error notification remains; standard suppression additionally prevents SDK
diagnostic conversion spans. The application retains exporter policy.

`FieldError::public(...).extension("errorId", ...)` cannot set the reserved key.
`Debug` output for internal `FieldError` and bridge/provider errors does not
expand sources. If error ID generation fails, masking and request semantics are
preserved and an ID-less internal event is sent to the observer. If request ID
generation fails, request and sampling semantics are also preserved, and
`ErrorObserver` receives an event with code `REQUEST_ID_GENERATION_FAILED`,
component `request-id-factory`, and no request or error ID. Error ID failure does
not recursively generate another ID: first it reports an ID-less
`ERROR_ID_GENERATION_FAILED` event for component `error-id-factory` in the same
phase, then reports the original failure without an error ID while preserving
client masking and response semantics.

### 3.2 Cardinality budget

A metric has at most eight labels; each label key and value is at most 64 bytes;
and provider/bridge names are compile-time static strings. Operation labels
accept only a bounded catalog-registered key or fixed-length digest. Label
constructors reject request IDs, error IDs, operation names, paths, principals,
tenants, and IP addresses.

## 4. Alternatives considered

### Implement observability only through the existing `Plugin`

Interceptors can modify results and short-circuit requests, yet cannot cover
transport and provider phases. Granting that much authority for simple
measurement is also excessive, so observers remain separate.

### Use the raw query as an operation label

This would leak secrets and create unbounded cardinality. ADR-0010 operation
keys are used instead.

### Make OpenTelemetry types part of the standard framework API

This would couple Pythia's core ABI to an external semantic convention and crate
version.

## 5. Consequences

### Positive

- A masked client error can be correlated safely with its internal source.
- One event model can observe endpoint, bridge, and provider request lifecycles.
- Core dependencies and semantics remain stable across telemetry backends.
- High-cardinality values and secrets are excluded by default.

### Negative

- The event schema and backend bridges require separate maintenance.
- The production application must explicitly configure the error ID factory and the exporter.
- Detailed field-level tracing is opt-in, and the application is responsible
  for its cost and cardinality.

## 6. Acceptance matrix

| ID | Expected |
| --- | --- |
| OBS-01 | Every lifecycle phase has consistent event ordering for success, rejection, timeout, and cancellation. |
| OBS-02 | Independent internal failures, including resolver, completion, and introspection failures, receive different error IDs; bubbles from one failure retain its ID; and factory failure emits an ID-less event. |
| OBS-03 | Direct and HTTP client error IDs correlate with observer sources, while the sources themselves remain absent from responses. |
| OBS-04 | A missing observer or failed sink does not change GraphQL response semantics. |
| OBS-05 | Raw queries, variables, headers, and personal data remain absent from default events, `Debug`, and metric labels. |
| OBS-06 | The closed, bounded metric-label set passes high-cardinality regression tests. |
| OBS-07 | A full exporter queue does not block requests and increments the drop count. |
| OBS-08 | Consumers with the OTel and tracing crates disabled gain no related dependencies. |
| OBS-09 | Disabling field instrumentation prevents events or spans proportional to the number of successful fields. |

Before moving to Accepted, public-contract-first review must fix the event
schema, reserved extensions, redaction fixture, cardinality budget, observer
panic policy, and OTel mapping version.
