# ADR-0004: Separate the HTTP transport pipeline from opt-in bridges and keep the server lifecycle in the application

Status: Accepted
Implementation: Implemented
Date: 2026-08-26
Last updated: 2026-10-24

Current product identity: the façade owner described below is
`chigiri::Endpoint`; former product spellings are retained as decision history.

2026-09-09 planned amendment: [ADR-0035](0035-cli-tui-and-embedded-graphiql.md) adds only an optional application-facing helper that assembles GraphQL and GraphiQL routes. That helper is the sole replacement for the route-construction prohibition below and the absence of a route API in DX-04. The neutral endpoint still handles an already-routed request, while the application retains ownership of listeners, runtimes, authentication, CORS, and shutdown. Existing handler/service evidence and this ADR's implementation status remain valid. The new helper requires separate DX35-ID acceptance and was not yet implemented by this amendment.

2026-10-24 implemented amendment:
[ADR-0055](0055-subscription-configuration-and-lifecycle-ergonomics.md) moves
managed GraphQL supervision and bounded stop/drain into the SDK, with automatic
fallible activation through thin official services and optional standard server
shutdown hooks. It replaces application-written driver/guard/reaper sequences,
not application-owned listeners, signal sources or unrelated server lifecycle.
The selected hook drains the SDK before signaling native server shutdown;
omitting the hook gives no coordinated drain guarantee. No server extension trait or
standalone server is added. This amendment is Accepted / Implemented with
local macOS evidence in the
ADR-0055 validation record.
The historical implementation evidence below retains its original source
scope; non-local platforms and production capacity remain unqualified.

## Context

GraphQL over HTTP's method, media type, status mapping and GraphQL request lifecycle are not dependent on the server framework.
Socket, routing, TLS, HTTP connection, signal, graceful shutdown are all server/runtime-specific for the application.

Initial v1 provided a reference server that `pythia_hyper::Server` owned up to the listener/accept loop.
The product boundary is not a server distribution; it is a generated resolver that processes the `/graphql` request.
The production endpoint/controller and the application server have explicit
ownership boundaries. ADR-0016 limits Hyper and Axum to opt-in bridges between
the application handler and the Pythia façade. `Endpoint<C>` refers to
`pythia::Endpoint<C>`; its 14-phase execution state belongs to the
façade-private transport pipeline.

## Decision

### Neutral endpoint

`pythia_http` defines transport-neutral request/response, codec, and
body/deadline/hook/lifecycle SPI. The `pythia::Endpoint<C>` façade-private
transport pipeline implements the following responsibilities.

- GraphQL request-lifecycle orchestration
- JSON POST request decode
- Query GET parameter decode
- `Accept` negotiation
- GraphQL/HTTP error status mapping
- The core `Value` and JSON conversion
- preflight, transport admission, CSRF, and Stage A policy before body polling
- bounded body read, source resolution, context construction, execution, and
  response-hook ordering with timeout/drop behavior

The endpoint owns no socket, route path, or HTTP connection. Its public API does
not expose Hyper, Axum, Tokio, or Tower types. Its JSON dependency is limited to
`serde_json`.

### Framework-neutral handler contract

The main entry point on `pythia::Endpoint<C>` accepts a bodyless
`TransportRequest<'a>`, a body implementing Pythia's body trait, and a
runtime-neutral deadline factory, and returns `ResponseParts`.

```rust
pub trait RequestBody: Send {
    type Chunk: AsRef<[u8]> + Send;

    fn next_chunk<'a>(
        &'a mut self,
    ) -> BoxFuture<'a, Result<Option<Self::Chunk>, BodyReadError>>;
}

pub trait DeadlineFactory: Send + Sync + 'static {
    fn sleep<'a>(&'a self, duration: Duration) -> BoxFuture<'a, ()>;
}

impl<C> Endpoint<C> {
    pub fn handle<'a, B>(
        &'a self,
        request: TransportRequest<'a>,
        body: B,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, ResponseParts>
    where
        B: RequestBody + 'a;
}
```

`handle` does not call `RequestBody::next_chunk` when method, media, or transport
admission can reject the request first. The endpoint applies body, context,
source, execution, and overall deadlines to their respective phases.
`DeadlineFactory` supplies only timer futures; it does not own phase ordering or
task spawning.

A buffered-body convenience constructor is available, but production bridges
use the streaming input above. `RequestBody::Chunk` permits zero-copy adapters
for types such as `bytes::Bytes` without fixing a framework-specific chunk type
in Pythia's public API.

### Opt-in Hyper/Axum bridges

`pythia_hyper` and `pythia_axum` are separate opt-in crates.

- Convert the framework request head to `TransportRequest`.
- Adapt the framework body as `RequestBody`.
- Adapt Tokio timers to `DeadlineFactory`.
- Convert `ResponseParts` into a framework response.
- Forward the peer address obtained directly from the framework and bounded
  metadata explicitly supplied by the application.

The bridge does not own listener binding, the accept loop, route construction,
TLS, HTTP-version policy, connection limits, signals, shutdown/drain, or
health/readiness endpoints. Pythia does not provide a standalone server API
such as `pythia_hyper::Server`, `pythia_server`, or `serve(address, shutdown)`.
The Hyper bridge is a service for application-owned Hyper, and the Axum bridge
provides the minimum glue needed to attach a handler to an application-owned
router.

The convenience surface supports the following usage patterns without leaking
framework types into `Endpoint` or the generated resolver contract.

```rust
let service = pythia_hyper::service(endpoint.clone());
// The application owns the listener, the connection builder, the shutdown and provides the service.

let handler = pythia_axum::handler(endpoint.clone());
let router = axum::Router::new().route("/graphql", axum::routing::any(handler));
```

Applications that do not use a bridge depend only on `pythia` and
`pythia_http`, and pass the same endpoint `RequestBody` and `DeadlineFactory`.
A dependency fixture verifies that Hyper, Axum, and Tokio do not enter this
direct-consumer dependency graph.

The existing `pythia_hyper::Server` is replaced by the bridge before production
release. Do not maintain the old server API and the new bridge concurrently
through a compatibility shim. Book examples keep listener and routing ownership
in application code so they cannot be mistaken for Pythia distribution
responsibilities.

### Application-owned server

The application owns the following:

- Async runtime activation and socket bind/accept
- `/graphql` routing, TLS, HTTP/1.1/2, CORS, compression
- header read timeout, connection/in-flight limit
- signal, graceful shutdown/drain, health/readiness routing
- Implementing `RequestBody` and `DeadlineFactory` when no bridge is used

Pythia owns request-lifecycle deadlines for body reading, context construction,
and execution. The application owns connection, header, and drain deadlines.
If the application applies a shorter outer deadline, its cancellation and
response behavior remains outside Pythia's conformance contract.

### HTTP feature scope

- POST: Query/Mutation
- GET: Query only.
- media: `application/json`, `application/graphql-response+json`
- unsupported: batch, multipart upload, WebSocket subscription, `application/graphql` body

The HTTP behavior is pinned to the Stage 2
[GraphQL over HTTP draft commit `eae19ff930de8b1cde8511b16843175b0e537e66`](https://github.com/graphql/graphql-over-http/blob/eae19ff930de8b1cde8511b16843175b0e537e66/spec/GraphQLOverHTTP.md).
The implementation must not follow a moving branch; changing the pinned commit
requires an explicit ADR update and review.

For `application/graphql-response+json`, Request Error statuses follow the
pinned draft's SHOULD: JSON syntax and GraphQL document parse failures use 400;
request-shape, GraphQL validation, operation-selection, and variable-coercion
failures inside well-formed JSON use 422. Legacy `application/json` retains 200
for GraphQL request errors for compatibility with existing clients.

Field errors and partial data are treated separately. The pinned draft is
internally inconsistent: its normative status-code section recommends a status
other than 200 for a response containing both `data` and `errors`, while its
later non-normative field-error example recommends 200. Version 1 chooses 200
to favor
interoperability with existing GraphQL clients and HTTP intermediaries and to
follow that example. The draft-aligned 400/422 rule above therefore applies
only to request errors.

Context construction and schema execution each have a separate default timeout
of 30 seconds. On expiry, the endpoint drops the pending future and returns 504
with `REQUEST_TIMEOUT`. A context future stalled on an authentication database
must not permanently occupy an application-server in-flight slot.

## Considered options

### Make Axum the public server API

This is the simplest option, but Tower/Axum extractor and router types would permeate the framework API and undermine replaceability and dependency minimization.

### Put Hyper directly in the core

This reduces the crate count but loses runtime-neutral execution.

### Provide a standalone Pythia reference server

This was the initial version 1 approach and gives users a short path to a running server, but it makes listeners, routing, connections, and shutdown framework responsibilities and conflicts with application server ownership. Runnable examples remain, but no server API is provided.

### Publish codecs without a common transport pipeline

This minimizes dependencies but would require every application to reimplement
phase ordering, pre-body policy, timeout/drop behavior, and status/media rules.
Instead, the Pythia façade privately owns the common transport pipeline and
leaves only framework conversion to bridges.

### Implement HTTP directly over raw Tokio TCP

This reduces dependencies, but HTTP parsing and connection management become a new security boundary outside a GraphQL framework's responsibility.

## Consequences

- Applications can replace the server stack without changing core/generated resolvers or `pythia::Endpoint`.
- Only users selecting the corresponding bridge depend on Hyper, Axum, or Tokio.
- Applications using Pythia directly can implement their own server/framework adapters.
- Listener, routing, connection, and shutdown policy stay unified with the application's operating environment.
- The codec owns GraphQL-over-HTTP status and media rules so adapters cannot reinterpret them independently.
- The pinned specification commit, 400/422 Request Error classification, 200 Field Error choice, and context timeout form a reproducible transport contract.
- Bridges and custom adapters must pass the same public testkit conformance suite.

## Revisit triggers

- Official distribution that owns the server is explicitly required.
- A serverless/Lambda bridge cannot be represented by the shared handler contract.
- The GraphQL-over-HTTP specification becomes Accepted and changes status or media semantics.

## Acceptance mapping

| Existing ID | Required evidence |
| --- | --- |
| DX-04 | The direct handler, Hyper service bridge, and Axum handler bridge all use the same unchanged `pythia::Endpoint`. |
| LIFE-02 | A preflight or transport rejection causes zero `RequestBody::next_chunk` polls. |
| LIFE-04 | A phase timeout drops the target future and body, and the bridge public API contains no listener or shutdown ownership. |
| LIFE-06 | The neutral endpoint public API exposes no Hyper/Axum/Tokio/Tower type, raw body/response, or route ownership. |
| TEST-01 | Direct, Hyper, and Axum paths run the same public lifecycle suite with no duplicated phase. |
