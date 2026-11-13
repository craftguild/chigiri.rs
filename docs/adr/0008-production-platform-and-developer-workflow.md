# ADR-0008: Make the schema-first developer workflow an invariant of the Pythia production platform

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md) and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md) define the Accepted / Planned next-phase generated-crate, CLI, configuration, ABI, and distribution boundaries. They supersede only the affected scope; existing GraphQL and security contracts and historical implementation evidence remain valid.

> 2026-09-09 planned amendment: [ADR-0035](0035-cli-tui-and-embedded-graphiql.md) adds an optional GraphQL/GraphiQL route helper. That helper alone replaces DX-04's absence of a route API; applications continue to own listeners, connections, and shutdown. DX-01 through DX-06 and their historical Passed result remain, while the new work is validated separately as DX35-01 through DX35-30.

Current product identity: the façade owner described below is
`chigiri::Endpoint`; former product spellings are retained as decision history.

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0001 through ADR-0007
- Coordinates: ADR-0004, ADR-0009 through ADR-0013, ADR-0016 through ADR-0018
- Generated ABI target: 5

ADR-0016 consolidated application endpoint construction under `pythia::Endpoint::builder`; ADR-0018 raised the generated ABI target to 5. This ADR's workflow and server-ownership decisions remain. References below to endpoint and ABI behavior use the replacement contracts from those two ADRs.

## 1. Context

Pythia originally generated resolver traits and a static schema descriptor from SDL, assembled application implementations through an ABI 3 typestate builder, and passed a runtime-neutral `ExecutableSchema` to HTTP handling. Current ABI 5 preserves that completeness, adds projection plans, and feeds a single endpoint builder directly.

Pythia is intended to become a production schema-first framework comparable in role to Apollo Server, not merely a migration aid for one product. Before adding rate limits, persisted operations, observability, and related features, the initial developer workflow and crate boundaries must be fixed as invariants.

The Guild GraphQL Code Generator divides responsibilities among plugins. Pythia's server-side resolver-trait generation is closest to [`typescript-resolvers`](https://the-guild.dev/graphql/codegen/plugins/typescript/typescript-resolvers). [`typescript-operations`](https://the-guild.dev/graphql/codegen/plugins/typescript/typescript-operations), which generates client result and variable types from schemas and operation documents, serves different users and inputs. Pythia uses the former as a server-codegen reference and excludes equivalent client codegen. ADR-0010 covers only server-side operation identity and optional registry/admission, not client artifacts.

## 2. Decision

### 2.1 Canonical developer workflow

The canonical workflow has six stages:

1. The application provides a canonical `schema.graphql` or schema directory.
2. `cargo pythia generate` validates SDL and generates Rust types, resolver traits, assembly APIs, and a static descriptor.
3. The application implements generated resolver traits using domain services, repositories, DataLoaders, and related components.
4. The application assembles every resolver group with generated `Resolvers::builder()`.
5. The application builds a framework-neutral `pythia::Endpoint` from the assembly. The same entry also accepts an already-built `ExecutableSchema`.
6. An application-owned server calls the endpoint from its `/graphql` handler, directly or through an opt-in bridge, and converts the neutral response into its HTTP response.

The data flow is:

```text
schema.graphql
  -> cargo pythia generate/check
  -> generated Rust contract
  -> application resolver implementations
  -> Resolvers<C, T>
  -> pythia::Endpoint<C>
  -> optional Hyper/Axum/other bridge
  -> application-owned router/server
  -> GraphQL over HTTP
```

SDL is the sole definition of the GraphQL API. Pythia never reconstructs the GraphQL schema from Rust types, resolver registration, operation registries, or observability configuration.

### 2.2 Generator responsibilities

The server generator emits one coherent artifact set containing:

- input objects, enums, OneOf types, and argument types;
- `SchemaTypes` and domain-binding points;
- resolver traits for every object, including roots;
- object-specific private dispatch adapters;
- ABI 5 resolver/projection assembly typestate;
- a validated static schema descriptor; and
- schema/configuration provenance and an ABI fingerprint.

It does not generate, discover, or distribute client operation result types, client documents, HTTP servers/listeners, rate-limit policies, or authentication. Applications choose their own client codegen, document placement, and client artifact format.

### 2.3 Resolver aggregation boundary

Stage 4 does not expose assembly as an application-built array. Applications call only source-ordered typestate methods; final `Resolvers<C, T>` privately owns a schema-ordered boxed slice.

This boundary preserves:

- compile errors for every missing resolver group or field method;
- no string-keyed runtime registry;
- generated-private assembly order, storage, and type erasure;
- freedom to split application resolver implementations across crates/modules; and
- no leakage of resolver-group internals into the public server API.

### 2.4 Execution, endpoint, and server boundaries

Pythia does not combine resolvers, execution, HTTP, and sockets into one server trait.

| Layer | Stable entry | Responsibility |
| --- | --- | --- |
| generated contract | `Resolvers<C, T>` | completeness between SDL and Rust implementations |
| execution service | `pythia::spi::ExecutableSchema<C>` | parsing, validation, coercion, execution, and GraphQL plugins; directly used only for prebuilt-schema input |
| GraphQL endpoint | `pythia::Endpoint<C>` | façade-private neutral HTTP lifecycle, body, deadlines, decode/encode, and policy composition |
| server integration | application and opt-in bridges | framework request/response conversion, routing, listeners, and connection lifecycle |

The stage-4 assembly is passed directly to `pythia::Endpoint::builder(resolvers)`. A prebuilt schema enters through the same `IntoEndpointInput` boundary. Application servers call the endpoint as a handler without knowing resolver storage or GraphQL execution internals. Bridge contracts use only Pythia-owned request-body, deadline, and response types; they include neither resolver nor HTTP-framework types. ADR-0004 and ADR-0009 define details.

### 2.5 Runtime and HTTP-framework replacement

- `pythia_core` and `pythia` do not depend on Tokio, Hyper, Axum, or Tower.
- `pythia_http` exposes no server-framework request/response type.
- The façade-private pipeline below `pythia::Endpoint` centralizes the 14 phases and core execution; `pythia_http` supplies codecs and neutral SPI contracts.
- Opt-in bridges convert framework request/body/timer/response values and never reimplement phases.
- The endpoint drives body streams and request-phase deadlines through Pythia-owned SPI.
- Applications own runtimes, sockets, routing, connection/header deadlines, and graceful shutdown.
- Application context owns principals, tenants, databases, DataLoaders, and transactions.
- Standard Pythia policies attach to explicit GraphQL-endpoint lifecycle phases.
- Pythia's public contract includes no standalone server, listener builder, or `serve(address, shutdown)`.

### 2.6 Placement of production features

| Category | Examples | Location |
| --- | --- | --- |
| framework invariant | lifecycle order, operation identity, budgets, masking, cancellation/drop | core, façade, and HTTP contracts |
| opt-in standard capability | rate limiter, persisted operations, OpenTelemetry bridge, testkit | independent crates |
| deployment/product policy | principal extraction, JWT/cookies, tenant quota, Redis, CORS origin, DB transactions | application/provider |

Do not add dependencies to consumers that do not use a standard capability. Keep provider-specific types out of core public APIs, and never select providers or production mode implicitly from environment variables.

## 3. Production-ready ADR program

This is the parent production-readiness decision. Detailed contracts are split as follows:

| ADR | Decision | Main output |
| --- | --- | --- |
| ADR-0009 | production request lifecycle and admission phases | pre-body/decoded/operation admission, deadlines, CSRF boundary |
| ADR-0010 | server-side operation identity and persisted operations | operation key, registry SPI, APQ, audit/safelist/ID-only |
| ADR-0011 | resource budgets and rate limiting | cost policy, two-stage limits, provider SPI, 429 contract |
| ADR-0012 | error identity and observability | error IDs, redaction, events/metrics, OTel bridge |
| ADR-0013 | schema change and release governance | schema/operation checks, testkit, release/security gates |

Dependency order is:

```text
ADR-0008
  -> ADR-0009
       -> ADR-0010
       -> ADR-0011
       -> ADR-0012
  -> ADR-0013 (integrates the public contracts from ADR-0009 through ADR-0012 into release gates)
```

ADR-0009 through ADR-0013 may become Accepted independently, but no release may claim production readiness before ADR-0013's integration gate passes.

### 3.1 Accepted crate and dependency direction

WP-P0 fixed these production-capability dependencies; reverse edges are prohibited.

```text
pythia_core <- pythia_runtime
pythia_core + pythia_runtime <- pythia_http
pythia_runtime + pythia_http + optional capability crates <- pythia (application façade)
pythia + pythia_http <- optional Hyper/Axum bridges
pythia_observability <- pythia_tracing / pythia_opentelemetry
all public contracts, endpoint and bridges <- pythia_testkit (consumer dev-dependency)
pythia_core <- pythia_codegen (including schema-governance CLI)
```

`pythia_core` contains only std-only identity/event values and traits, never SHA-256, clocks, queues, runtime timeouts, HTTP, or provider implementations. `pythia` may normally depend on `sha2` because every request needs an exact document SHA-256, but it does not depend on registry, rate, or telemetry backends.

### 3.2 Generated ABI decision

WP-P1 advanced the generated contract from version 3 to version 4 when canonical
schema digests were added to generated descriptors, allowing `SchemaInfo`,
operation identity, and readiness to share one value. ADR-0018 later advanced
ABI 4 to ABI 5 for sealed projection input and private layout. Current runtime
constants, generators, golden output, books, the large consumer, and every
checked-in source use only ABI 5; there is no old-ABI reader or mixed mode.

## 4. Cross-ADR invariants

Subsequent ADRs must preserve:

- SDL-first, checked-in deterministic generation;
- compile-time resolver completeness;
- the 400-physical-line hard limit for generated code as historically specified, subject to ADR-0028's current ownership/layout rules;
- a std-only, runtime-neutral, unsafe-free `pythia_core`;
- Query concurrency, serial Mutation roots, and null/error propagation;
- centralized GraphQL-over-HTTP lifecycle in `pythia::Endpoint` with `pythia_http` codec/SPI boundaries;
- dropping timed-out futures and never detaching tasks;
- default internal-error masking;
- no bridge/provider dependency for consumers that do not select it; and
- application ownership of listeners, routing, connections, and shutdown.

Any change requires amending this ADR and affected ADRs together.

## 5. Explicit non-goals

The first production-readiness phase excludes:

- application-specific principals, IPs, cookies, database schemas, and quota values;
- runtime resolver registration or partial resolver implementation;
- generation of arbitrary domain-service implementations from schema;
- mixing browser/client SDK generation into the server generator;
- runtime SDL hot reload—schema changes require generation, compilation, and binary rollout; and
- Federation, Subscription, multipart upload, `@defer`, and `@stream`.

Evaluate these in separate ADRs after the production-platform gates are complete.

## 6. Alternatives considered

### Put every feature in one production ADR

Lifecycle, security, observability, and release rationales and acceptance oracles would become inseparable and hard to implement or review independently.

### Standardize an external application's middleware stack as the server API

This simplifies migration for one product but freezes authentication, rate limiting, cookies, and proxy trust in one product-specific order. Repo-owned synthetic fixtures cover the framework boundary instead.

### Provide a standard standalone Pythia server

The initial Hyper reference server shortened quick start but made listeners, routing, connections, signals, and shutdown part of the framework contract. Runnable servers remain consumer examples only.

### Copy The Guild's plugin structure exactly

TypeScript resolver maps, module systems, and client document types differ from Rust trait completeness, associated types, and ABIs. The Guild informs the workflow, but Pythia public contracts follow Pythia invariants.

## 7. Consequences

### Positive

- The path from SDL to a framework-neutral endpoint remains singular as production features are added.
- Server codegen and client tooling remain separate.
- Crate boundaries preserve runtime, HTTP, and provider choices.
- Applications compose their existing server operations with Pythia's GraphQL lifecycle without duplication.
- Repo-owned full-schema fixtures exercise the public API without importing application policy.

### Negative

- Production readiness spans multiple ADRs and releases.
- Providing persisted operations requires maintaining a server-side registry SPI.
- Every bridge and custom application integration needs shared lifecycle conformance tests.
- Application-owned-server quick starts require more visible glue than a standalone API.

## 8. Acceptance criteria

| ID | Required evidence |
| --- | --- |
| DX-01 | An application-owned-server quick start reaches POST Query/Mutation and GET Query using only the six stages in Section 2.1. |
| DX-02 | Adding an SDL object or field produces a compile error for a missing resolver implementation or assembly step. |
| DX-03 | Application source does not specify `Resolvers<C, T>` storage. |
| DX-04 | Hyper/Axum bridges and the bridge-free neutral handler share one `pythia::Endpoint`, and bridges expose no listener, route, or shutdown API except the later ADR-0035 route helper. |
| DX-05 | Server resolver codegen works without Pythia client tooling or a client artifact format. |
| DX-06 | Framework public APIs contain no application-specific type, header, database, or quota value. |

Design review confirmed that ADR-0009 through ADR-0013 do not overlap and that each acceptance ID has an owning test/documentation gate. Implementation completion is determined by the integrated current-ABI-5 and single-façade-ownership gate.
