# ADR-0009: Define the production request lifecycle, including transport admission

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-27
- Decision owners: Pythia maintainers
- Depends on: ADR-0004, ADR-0008
- Revisits: ADR-0003, ADR-0004, ADR-0007

Current product identity: the façade owner described below is
`chigiri::Endpoint`; former product spellings are retained as decision history.

## 1. Context

Pythia already has Request, Operation, and Field plugins in the core, plus
request, context, and response hooks in the Axum adapter. Production use also
requires coarse rate limiting before the body is read, persisted-operation
resolution, operation-cost admission, and shared telemetry. Their positions and
relative order need one explicit contract.

Without that contract, Hyper and Axum adapters can apply authentication, rate
limits, timeouts, and error conversion in different orders even when they use
the same schema and policy. Moving HTTP metadata or a body stream into a core
plugin would instead violate the runtime-neutral core boundary.

## 2. Decision

### 2.1 Lifecycle ownership

The lifecycle begins after the application router forwards a request to the
Pythia endpoint.

| Owner | Responsibility |
| --- | --- |
| application server or ingress | runtime, socket, TLS, proxy trust, CORS, routing, header timeout, connection control, and shutdown/drain |
| optional framework bridge | conversion between framework request/body/timer/response types and Pythia-owned SPI types |
| `pythia::Endpoint` façade-private transport pipeline | preflight, transport admission, bounded body read, decode, persisted-source resolution, request deadlines, and response encoding |
| Pythia execution | parse, validation, selection, coercion, operation admission, and field execution |

If application middleware replaces a Pythia response, the resulting behavior is
outside Pythia's compliance boundary. Route mismatches, connection rejection,
header timeout, and refusal of new requests during shutdown are not converted
into Pythia responses.

### 2.2 Normative request order

A request forwarded from an application-owned router to
`pythia::Endpoint::handle` proceeds in this order:

```text
1. HTTP method/media preflight
2. transport admission
3. bounded body read
4. HTTP request decode
5. operation source resolution
6. decoded-request hook
7. application context construction
8. core Request plugin
9. parse or validated-document cache lookup
10. validation, operation selection, variable coercion
11. core Operation plugin / operation admission
12. field collection, Field plugin, resolver execution
13. GraphQL response observer/hook
14. HTTP encode and permitted header append
```

Inapplicable phases are omitted, but the relative order never changes. For a
normal request that already contains a raw query, phase 5 performs identity
checks only. Direct execution of a `GraphQlRequest` starts at phase 8 and
therefore omits phases 1 through 7 and 13 through 14.

ADR-0010 owns operation-source resolution. Invalid IDs, hash mismatches, and
safelist rejections terminate before parsing. After resolution, phase 8 and
later receive the complete GraphQL document text as they did before this
decision.

### 2.3 Admission phases

Two interception points are standardized.

#### Transport admission

Phase 2 runs before the endpoint polls the request body. It may inspect only:

- the method, URI, and validated header names and values;
- the negotiated response-media candidate;
- the peer address obtained directly by the bridge or application server; and
- bounded metadata explicitly supplied by the application.

Transport admission cannot inspect the raw body, GraphQL document, variables,
or application context `C`. Pythia never interprets `Forwarded` or
`X-Forwarded-For` automatically. The application or ingress must apply an
explicit trusted-proxy policy and pass only bounded, validated metadata.

#### Operation admission

Phase 11 may inspect:

- operation identity, kind, and name;
- schema digest;
- metadata derived from coerced variables, with disclosure of actual values
  requiring an explicit policy;
- the complexity and resource summary; and
- principal or tenant keys that the application extracted into context `C`.

This phase owns per-operation authorization, cost and quota policy, and safelist
auditing. Field plugins must not be repurposed for transport admission.

### 2.4 Shared endpoint contract

`pythia_http` defines the framework-neutral types needed for transport
admission, decoded-request hooks, and typed rejection. Its public signatures do
not expose Hyper or Axum `Request`, `HeaderMap`, or `StatusCode` types.

The façade-private pipeline in `pythia::Endpoint` uses ADR-0004's
`RequestBody` and `DeadlineFactory` contracts and runs the 14 phases exactly
once. A framework integration converts the request into a borrowed neutral view
and implements only body, timer, and response conversion. Hooks may return
boxed `Future + Send` values, but they do not own timers or spawn tasks. The
endpoint polls a deadline future in the request task and drops both the deadline
and target future when the phase completes or times out.

The existing `pythia_axum::RequestHook` lacks this complete timing and ownership
contract and therefore migrates to the common decoded-request hook before the
next release. Pythia does not run old and new hooks as duplicate pipelines. A
transition type alias is allowed only when the semantics are identical. In the
same release, the handler/service bridge replaces `pythia_hyper::Server`; no
listener API compatibility shim is provided.

### 2.5 Interceptors and observers

- An interceptor may continue the pipeline, return a typed rejection, or
  report an internal failure.
- An observer cannot change the result; except for a panic, it cannot affect the
  request outcome.
- Existing onion plugins continue to perform Request, Operation, and Field
  transformation.
- Metrics and logging are not implemented as short-circuiting plugins.

ADR-0012 defines the observer event and redaction contract.

### 2.6 Deadlines and cancellation

The endpoint calculates an overall deadline at entry. Each asynchronous phase
uses the earlier of that deadline and its phase-specific deadline, preferring
the phase-specific deadline if both expire simultaneously. Body read,
transport/decoded hooks, source resolution, context construction, execution,
and the response hook have separate timeout categories.

- On timeout, the target future or body stream is dropped.
- The endpoint and bridge do not spawn a detached timeout task.
- Resolver, application, and provider futures use cooperative cancellation on
  drop.
- A timeout or response-hook failure does not roll back mutation side effects
  that have already completed.

### 2.7 Rejection and failure responses

A typed rejection contains a validated HTTP status from 400 through 599, a
client-safe message, a stable application code, publishable extensions, and
optional bounded retry metadata. It cannot supply a raw response body. The
endpoint encodes it as a GraphQL request error using the negotiated media type.

Internal hook or provider failures are masked as HTTP 500 with
`INTERNAL_SERVER_ERROR`. ADR-0004 continues to own the other status/code
mappings; ADR-0011 adds the 429 rate-limit contract.

The response hook receives data, errors, status, media type, and codec
information as read-only values. It cannot replace the status, body, media type,
or codec-owned headers. It may append only headers that pass the end-to-end
header validation contract.

### 2.8 Browser transport security

Pythia provides a standard CSRF/XS-Search admission policy that requires a
preflighted request. A browser simple GET is rejected unless it carries the
required non-simple request header.

Allowed origins, credentials, and private-network policy are deployment
specific and remain owned by the application or ingress. Development may relax
the CSRF policy only through an explicit builder setting; environment variables
never disable it implicitly.

## 3. Public contract constraints

The implementation spike determines final Rust names, but the Accepted public
contract must satisfy all of these constraints:

- Neutral request metadata does not own the body.
- `RequestBody::next_chunk` is not polled until preflight and transport
  admission succeed.
- `DeadlineFactory` supplies only a timer future; it does not run phases, spawn
  tasks, or modify responses.
- Header values are not converted implicitly into unbounded `String` values.
- Direct peer addresses and proxy-derived client identities use distinct fields
  and types.
- `Next` and phase tokens are single-use and cannot be cloned.
- Rejection construction validates both the status range and reserved headers.
- Observer and interceptor traits remain distinct.
- The endpoint exposes a test seam that proves no phase runs more than once.
- Bridge public APIs do not contain a bind address, listener, router
  construction, connection limit, or shutdown future.

### 3.1 Accepted Rust contract

The WP-P0 compile spike fixes these owning crates and public concepts:

- `pythia_http::TransportRequest<'a>` is a borrowed view of method, URI,
  headers, direct peer information, and bounded application-supplied metadata.
  It contains no body, GraphQL query, variables, or framework extension map.
- `pythia_http::TransportAdmission` is an object-safe admission SPI that returns
  a boxed `Future + Send`.
- `pythia_http::DecodedRequestHook` receives a read-only `GraphQlRequest` view
  after source resolution and may return a typed rejection.
- `pythia_http::Rejection` contains a validated 400-through-599 status,
  client-safe error, and optional validated retry metadata.
- `pythia_http::OperationSourceResolver` is an object-safe SPI that resolves a
  decoded envelope into a complete operation source.
- `pythia_http::RequestBody` is a streaming trait with an associated chunk type.
- `pythia_http::DeadlineFactory` is an object-safe SPI that supplies only
  runtime timer futures.
- `pythia::Endpoint<C>` owns `ExecutableSchema<C>` and the façade-private
  transport lifecycle, and advances a neutral request to a response once.
- `pythia_core::LifecycleObserver` and `ErrorObserver` are synchronous,
  return-free observers and are distinct from interceptors.

Async traits preserve borrow lifetimes in their returned futures. Shared
providers are `Sync + 'static`, and their futures are `Send`. Dropping the task
drops the active future. `Debug` output for `TransportRequest` and observer
events does not expose header values, URI query values, or application metadata
values.

### 3.2 Status, media, and header authority

Typed rejection, source-resolution failure, rate-limit rejection, and every
other early exit become `GraphQlResponse::request_error` values encoded using
the negotiated media type. Applications cannot supply a raw body or override
`Content-Type`, `Content-Length`, or hop-by-hop headers. `Retry-After` is
appended only when the endpoint can render validated nonnegative seconds as
decimal. The old and new pipelines are never both invoked for one request.

## 4. Compatibility and migration

- Existing core Request, Operation, and Field plugin order and onion semantics
  remain unchanged.
- Direct `GraphQlRequest` execution starts at phase 8.
- The Axum adoption example moves product-owned processing into the common hook
  and application context.
- The books example keeps listener, routing, and shutdown logic in application
  code rather than the Hyper bridge.
- Hyper and Axum bridges run the same endpoint lifecycle conformance suite.
- Applications that used bridge-specific extensions migrate to explicit
  extraction during transport admission.

## 5. Alternatives considered

### Add every phase to the core `Plugin`

Rejected because HTTP metadata and body streams would leak into the core and
would have no coherent meaning for direct execution.

### Standardize only Tower middleware

Rejected because it would make Axum the only natural integration and would not
unify Hyper or future bridges with operation and field phases.

### Keep the Hyper reference server

Rejected because listener, connection, and shutdown ownership would overlap the
application server. The Hyper integration is limited to request, body, timer,
and response conversion.

### Provide one all-powerful request hook

Rejected because available data, trust, and resource cost differ before the
body is read, after source resolution, and after operation validation.

## 6. Consequences

### Positive

- The endpoint defines one sequence from coarse rate limiting through field
  execution.
- Admission can reject untrusted requests before reading their bodies.
- Production policy can be added without importing HTTP-specific types into the
  core.
- Timeout and cooperative-drop guarantees apply consistently to providers.
- Applications retain their existing routing, connection, and shutdown policy.

### Negative

- Axum hook APIs and Hyper server APIs require a pre-release migration.
- Each bridge must implement body/timer conversion and pass the conformance
  suite.
- Applications must distinguish a direct peer address from a proxy-derived
  client identity explicitly.
- Applications that do not use an official bridge must implement Pythia's
  `RequestBody` and `DeadlineFactory` contracts.

## 7. Acceptance matrix

| ID | Expected evidence |
| --- | --- |
| LIFE-01 | The neutral endpoint and both Hyper and Axum bridges use the phase sequence in Section 2.2. |
| LIFE-02 | A transport-admission rejection causes zero `RequestBody::next_chunk` polls. |
| LIFE-03 | A source, decoded-hook, context, or operation rejection does not invoke the next phase. |
| LIFE-04 | A phase timeout drops the target future or body, spawns no detached task, and adds no listener or shutdown API to a bridge. |
| LIFE-05 | Direct execution preserves core plugin order while omitting HTTP phases. |
| LIFE-06 | The common endpoint API contains no raw body, raw response, trusted-proxy inference, or server-lifecycle type. |
| LIFE-07 | The production CSRF policy rejects browser simple requests and permits requests with the explicit preflight header. |
| LIFE-08 | The response hook cannot replace the status, body, media codec, or codec-owned headers. |

Before implementation is marked complete, a compile fixture must fix the open
type skeleton, tests must cover every failure-path status/code and phase owner,
and the results must be traced in the specification-conformance matrix.
