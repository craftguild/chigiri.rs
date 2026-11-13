# ADR-0016: Start a single Endpoint builder from a safe baseline

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0004, ADR-0006, ADR-0008 through ADR-0013
- Revisits: `pythia` façade, `pythia_production::ProductionPreset`, and the
  generated ABI 4 schema-construction path
- Coordinates: ADR-0017 through ADR-0019

Current product identity: the façade owner described below is
`chigiri::Endpoint`; former product spellings are retained as decision history.

2026-09-09 planned amendment: [ADR-0035](0035-cli-tui-and-embedded-graphiql.md)
adds `graphiql(bool)` to the same Endpoint builder. It remains false by default;
only true is explicit consent to expose the UI and its introspection dependency.
This is a narrow addition to Section 2.5's `dangerously_*` rule and does not
authorize other security relaxations or a second builder. The final GraphiQL
setting remains separate from independent introspection configuration. Turning
GraphiQL off again removes only GraphiQL-originated exposure and rationale.
Server ownership remains unchanged except for the route helper. This new
contract is Planned; existing Implemented/Passed evidence does not transfer to
its new acceptance items.

## 1. Context

The current standard path assembles generated `Resolvers`, generated
`ExecutableSchemaBuilder`, `ProductionPresetBuilder`, and
`GraphQlEndpointBuilder` in sequence. Their responsibilities can be separated,
but users must duplicate policy manually between schema and endpoint and decide
which builder is the entry point. Even the books example connects several
factories, observers, cost policy, two-stage rate limiting, and lifecycle hooks
separately for production configuration.

An API is unsafe when forgetting `production_defaults()` alone makes execution
unbounded or exposes information. Yet Pythia also cannot infer principals,
trusted proxies, tenant quotas, or distributed stores for every application.

Pythia therefore contains complexity internally and presents ordinary users
with one type-safe construction entry point.

## 2. Decision

### 2.1 Public construction entrypoint

The standard application entry point for building a GraphQL endpoint is the
single `pythia::Endpoint::builder` API. Parallel entry points such as
`Endpoint::advanced`, `ProductionEndpoint::builder`, or
`Endpoint::development` are not added.

Generated `Resolvers<C, T>` retains the schema descriptor, resolver ABI
fingerprint, and dispatch as a private contract, so it is sufficient input to
the endpoint builder.

```rust,ignore
let resolvers = generated::Resolvers::<AppContext, Types>::builder()
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_user(UserFields)
    .build();

let endpoint = pythia::Endpoint::builder(resolvers)
    .context_factory(AppContextFactory::new(state))
    .build()?;
```

The same builder also accepts an already constructed `ExecutableSchema<C>`
through the same `IntoEndpointInput<C>` boundary; this is not a separate
advanced entry point.

```rust,ignore
let endpoint = pythia::Endpoint::builder(executable_schema)
    .context_factory(factory)
    .build()?;
```

An already built schema contributes only its schema identity, semantic schema,
resolver dispatch, custom-scalar registration, and catalog revision. Limits,
caches, introspection, plugins, samplers, lifecycle/error observers, ID
factories, and field instrumentation configured on the lower-level schema are
not inherited as a second component graph.
The final policy is rebuilt from the same endpoint draft as the automatic
baseline; required components move to sugar methods or `inject` on that builder.
Direct execution of the constructed schema outside `Endpoint` retains its
existing configuration.

`Endpoint<C>` is a cheap-to-clone handle for the neutral endpoint. It exposes
direct execution, neutral `handle`, schema information, and in-flight count.
Hyper and Axum bridges consume that same `Endpoint<C>`. Listeners, routing,
sockets, runtimes, signals, and shutdown remain application-owned, and no
standalone server API is added.

### 2.2 Façade dependency direction

Because the current `pythia` crate is also the lower-level runtime, it cannot yet
serve as an upper façade that re-exports `pythia_http`. Implementation reorganizes
the dependency direction as follows:

```text
pythia_core <- pythia_runtime
pythia_runtime + pythia_core <- pythia_http
stable runtime/HTTP SPI <- pythia_operation / pythia_policy / pythia_observability

pythia_runtime + pythia_http + standard capability crates
  <- pythia (application-facing façade)

pythia + pythia_http
  <- opt-in pythia_hyper / pythia_axum
```

The dependency provider is on the left of each arrow and the consumer on the
right. Operation, policy, and observability crates do not depend on one another;
they depend only on the minimum stable runtime/HTTP SPI. The upper façade owns
composition.

The runtime implementation in `crates/pythia` moves to `pythia_runtime`, while
the `pythia` package becomes the application-facing façade. Generated sources
continue to refer only to `pythia`; the façade re-exports runtime values,
resolver SPI, and the generated private ABI.
`pythia_production::ProductionPreset` composition is integrated into the new
builder rather than retained as another required construction path.

Lower-level crates exist for independence and internal dependency direction.
The façade's private module owns the 14-phase HTTP execution state;
`pythia_http` exposes no endpoint handle or builder. README quick starts and
generated rustdoc do not present `pythia_runtime` or `pythia_http` as application
entry points. Only users implementing a bridge reach neutral codec/SPI contracts
through `pythia::spi`.

The project has not shipped a production release, so this crate reorganization
provides no compatibility shim for old import paths or `ProductionPreset`.

### 2.3 Automatic safe baseline

Every `Endpoint::builder` starts with the following baseline. Users do not call
`production_defaults()`.

| Area | Automatic baseline |
| --- | --- |
| execution | `ExecutionLimits::production()`, Query concurrency 64, list 1,000, output node 100,000 |
| document | 256 KiB, depth 32, expanded fields 1,000, aliases/variables/directives 100 each |
| cache | first limit reached: 1,024 entries or 16 MiB of queries |
| schema visibility | introspection off, suggestion off |
| errors | always mask internal details and attach a random error ID |
| request identity | random request IDs derived from a process seed; field instrumentation off |
| HTTP body | request 1 MiB, response 8 MiB |
| deadlines | body, context, execution, and overall: 30 seconds each |
| browser admission | ADR-0009 production CSRF/XS-Search policy |
| operation source | raw document required; persisted operations disabled |
| rate limit | unconfigured without inferring product identity; structural limits always active |
| observers/hooks | no-op without result interference or response-header additions |

`Endpoint::builder` retains only an uninitialized plan for automatic request IDs
until final `build()`. A prefork server may prepare the builder in the parent,
but each worker calls `build()` after forking and before accepting a request. If
the baseline cannot be constructed safely, `build()` fails rather than selecting
a weaker request-time fallback. A constructed endpoint is bound to its creator
process and is not carried into a child after fork.

An unconfigured rate limiter does not mean unbounded execution.
Framework-owned resource boundaries—body, document, cost, list, output, and
deadlines—remain active. Principal-, tenant-, and multi-instance quotas are
application-specific, so `Endpoint::configuration()` and readiness summaries
report `RateLimitCoverage::Unconfigured`; they never imply distributed
protection.

Normal setters allow at most 1 GiB for request bodies and encoded responses and
24 hours for each request phase and the overall deadline. Those boundary values
are valid, but larger values and unbounded sentinels such as `usize::MAX`,
`Duration::MAX`, or zero fail final validation. If a runtime adapter cannot
represent deadline addition, it fails to an immediate timeout rather than an
indefinite wait. No dangerous API for removing these limits is currently
published.

### 2.4 Context and runtime-owned requirements

There is no safe default for arbitrary `C`, so typestate requires a
`context_factory` whenever `C != ()`. Only `C = ()` may use unit context, and
only after an explicit `.without_context()`. Pythia does not infer context,
runtime, or development mode from environment variables.

At request execution, a bridge or application supplies the `DeadlineFactory`.
Neutral `handle` and timed direct `execute` accept one as an argument; Hyper and
Axum bridges supply their opt-in runtime adapters; custom bridges are
application-implemented. Rate-provider and other phase timeouts use the same
request deadline source. Builders and presets retain no separate runtime timer.
This argument is a runtime-substitution boundary, not a second endpoint
construction input.

### 2.5 Defaults are a public behavioral contract

Defaults are release contracts, not implementation details. Except for security
fixes, changing a default so that a previously successful request can be
rejected requires a migration note, a before/after test, and a release note. An
emergency security tightening still requires an advisory and recorded rationale.

Operations that weaken safety—removing limits, enabling introspection, disabling
CSRF protection, or exposing internal details—do not use ordinary
`Option::None` or zero values. They are limited to explicitly named APIs such as
`dangerously_disable_*`. Pythia does not describe them with Rust's memory-safety
term `unsafe`.

## 3. Build semantics

The builder creates one immutable component graph in baseline, user-override,
and final-validation order.

```text
automatic safe baseline
  -> sugar / inject overrides in call order
  -> final cross-component validation
  -> immutable ExecutableSchema + neutral Endpoint
```

At minimum, `build()` validates ABI/schema fingerprints, every limit and
deadline, minimum response capacity, rate-policy/provider compatibility,
reserved headers, and ID-factory initialization. Failure never returns a
partially constructed endpoint.

This final stage initializes the automatic request-ID factory in the worker and
passes its first generated value to the first request through a lock-free
one-shot. The official factory and Endpoint wrapper verify their creator process
ID and fail closed when used from another process instead of falling back to a
duplicate ID.

## 4. Non-goals

- Do not infer application-specific JWT, cookie, tenant, proxy-trust, or Redis
  topology.
- Automatic defaults are not a security certification of the complete production
  deployment.
- Do not put the server, listener, or runtime in the façade.
- Do not add client operation code generation.
- Do not regress to a runtime resolver registry or partial resolver completeness.

## 5. Acceptance matrix

| ID | Expected |
| --- | --- |
| API-01 | The quick start reaches Query, Mutation, and GET using only `Resolvers::builder`, `Endpoint::builder`, a context factory, and a bridge. |
| API-02 | Section 2.3's framework-owned baseline is active with no public `production_defaults()` call. |
| API-03 | Generated resolvers and an existing `ExecutableSchema` both enter the same `Endpoint::builder`; built-schema input does not inherit its former execution components. |
| API-04 | No second entry point such as `Endpoint::advanced` appears in the public API, rustdoc, or README. |
| API-05 | Missing context for `C != ()` is a compile failure; unit context is constructed only with explicit `.without_context()`. |
| API-06 | ID-factory initialization or final configuration-validation failure causes `build()` to fail closed. |
| API-07 | Every structural limit remains active when rate limiting is unconfigured, and effective configuration reports rate coverage as unconfigured. |
| API-08 | Direct, neutral, Hyper, and Axum paths use the same endpoint, schema digest, and defaults. |
| API-09 | The public façade quick start does not expose manual assembly through `pythia_runtime`, `pythia_http`, or provider glue. |
| API-10 | `pythia_core` remains std-only and the application continues to own the server lifecycle. |
| API-11 | Safety-weakening behavior is reachable only through explicit dangerous APIs. |
| API-12 | The books example requires less application code than its previous manual preset assembly while preserving the same response and security contracts. |

## 6. Consequences

### Positive

- Users learn one builder and obtain a minimally configured safe endpoint.
- Production configuration cannot be omitted, and policy is not duplicated
  between schema and endpoint.
- Façade IDE completion leads continuously from ordinary to advanced settings.

### Negative

- Crate dependency direction and package configuration require a one-time migration.
- The application-facing `pythia` package has more default dependencies than the
  current runtime façade.
- Changing an automatic baseline becomes an explicit compatibility decision.

## 7. Rejected alternatives

- Require `production_defaults()`: forgetting the call changes security, while
  remembering it adds a ritual line even to minimal usage.
- Add `Endpoint::advanced`: this splits IDE suggestions and the learning path.
- Automatically configure an anonymous global rate limit as a production
  guarantee: this misrepresents proxy, tenant, and multi-instance semantics.
- Hide the four existing builders only in documentation: policy duplication and
  multiple type-level entry points would remain.
