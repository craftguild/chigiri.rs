# ADR-0007: Add an adoption profile for existing large Axum products

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md)
> and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> define the Accepted/Planned next-phase generated-crate, CLI, configuration,
> ABI, and distribution boundaries. They supersede only that scope. The GraphQL,
> security, and historical implementation evidence in this ADR remain valid.

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-27
- Last updated: 2026-08-29
- Decision owners: Pythia maintainers
- Revisits: ADR-0002, ADR-0004, ADR-0005
- Target: vNext / generated ABI 3

ADR-0004's 2026-08-27 handler-only revision makes the Hyper reference server in
this ADR historical comparison material. Socket binding, routing, and graceful
shutdown belong to the application/Axum server. `pythia_axum` remains an opt-in
handler bridge.

ADR-0014's 2026-08-29 correction supersedes Section 9's former unconditional
ban on response-subtree roots. Whole-`/body` replacement remains forbidden, but
exact differential markers under `/body/data`, `/body/errors`, and
`/body/extensions` are allowed under ADR-0014's conditions.

## 1. Normative language and completed state

The words **must**, **must not**, **required**, and **forbidden** are normative.
A recommendation may be changed only when the reason for deviating is recorded
in the design documentation for the same change.

`Status: Accepted` records approval of the design. `Implementation: Implemented`
records that the repository and checked-in generated output completed the ABI 3
acceptance gates. The completed transition was:

1. Accept this ADR and synchronize architecture, conformance, and handoff docs.
2. Complete WP-V0 through WP-V6.
3. Mark implementation complete only after the final fresh-consumer gate.

There is no compatibility shim for ABI 2 runtime or generated source; consumers
must regenerate for ABI 3. Do not mix only some work packages into the ABI 2
public contract.

## 2. Context

A historical application evaluation examined large-schema Axum integration,
DataLoader, authentication, rate limiting, cookies and schema-first client
generation. It used an isolated consumer for generation, compilation and linting.
The observations below retain their historical scope; they do not qualify the
current synthetic fixtures or product source.

| Item | Observation |
| --- | --- |
| SDL | 1,504 lines / 30,643 bytes |
| Schema | 67 objects, 19 input objects, 65 enums, 5 custom scalars |
| Resolver surface | 67 traits, 709 methods, 46 argument structs |
| Generated output | 290 `.rs` files / 95,484 lines / at most 350 lines per file |
| Pythia type boundary | 70 `SchemaTypes` associated types |
| Existing corpus | About 40 backend execution documents and 48 frontend operation documents |

The unmodified SDL was rejected with `SF2004` because async-graphql's printer
included definitions for the specification's built-in `@include` and `@skip`
directives. Temporarily removing only those two definitions allowed generation
and drift checking to succeed. A standalone consumer compiled on Rust 1.85,
after which Clippy exposed two generated-code problems: an empty public glob
re-export from a shard with no public items and the 67-argument
`GeneratedResolvers::new` constructor.

The product's enum vocabulary was deliberately shared across its database,
domain Rust types, GraphQL, TypeScript, and hash inputs. Existing object and
custom-scalar mappings fit `SchemaTypes`, but enum mappings required an explicit
opt-in contract. The existing Axum middleware and handler also owned body and
overall limits, IP/session rate limiting, header- and connection-based
authentication, per-request DataLoader construction, query-based rate
classification, post-execution diagnostics and `Set-Cookie`, and composition
with existing routing, state, and graceful shutdown. A product-specific handler
could express this through `pythia_http`, but reimplementing the codec lifecycle
per product would fork media, status, timeout, and error semantics. The former
Hyper reference adapter alone was not a sufficiently low-risk Axum migration
path. These observations satisfied the relevant revisit triggers in ADR-0002,
ADR-0004, and ADR-0005.

## 3. Decision overview

The existing-product adoption profile added seven capabilities:

1. warning-free builds in an ordinary consumer crate;
2. normalization of equivalent GraphQL specification directive definitions;
3. a linear type-state resolver assembly builder instead of a positional
   constructor;
4. opt-in domain enum bindings;
5. a separate `pythia_axum` adapter crate;
6. structural differential migration as the standard cutover method; and
7. reproducible distribution of runtime, generator, and generated ABI.

The following ADR-0001 through ADR-0006 principles remain unchanged:

- SDL is the GraphQL API source of truth.
- Every object resolver must be complete at compile time.
- `pythia_core` has no runtime, HTTP, or Serde dependency.
- `pythia_http` owns HTTP codec semantics.
- Internal errors expose only a fixed client message and code.
- Handwritten Rust and shell files remain within the repository line limit.

## 4. Consumer generated-code quality contract

Generated output must pass the same standard checks as handwritten consumer
code:

```text
cargo +1.85.0 check --all-targets --all-features
cargo +1.85.0 clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo +1.85.0 doc --no-deps --all-features
cargo +stable check --all-targets --all-features
cargo +stable clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo +stable doc --no-deps --all-features
```

Consumer-selected `clippy::pedantic` and `clippy::nursery` are outside the
guarantee. A generated item may carry a narrowly scoped default-Clippy allow
only when no structural alternative exists. Module-wide and crate-wide allows
are forbidden. Empty public glob re-exports are omitted rather than suppressed.

## 5. SDL built-in directive import normalization

### 5.1 Closed built-in set

The importer recognizes exactly these GraphQL September 2025 definitions:

| Name | Arguments | Repeatable | Locations |
| --- | --- | --- | --- |
| `skip` | `if: Boolean!` | false | `FIELD`, `FRAGMENT_SPREAD`, `INLINE_FRAGMENT` |
| `include` | `if: Boolean!` | false | `FIELD`, `FRAGMENT_SPREAD`, `INLINE_FRAGMENT` |
| `deprecated` | `reason: String! = "No longer supported"` | false | `FIELD_DEFINITION`, `ARGUMENT_DEFINITION`, `INPUT_FIELD_DEFINITION`, `ENUM_VALUE` |
| `specifiedBy` | `url: String!` | false | `SCALAR` |
| `oneOf` | none | false | `INPUT_OBJECT` |

This is a closed list, not a prefix or extensible class.

### 5.2 Equivalence algorithm

For a source definition whose name is in the table:

1. First perform the ordinary AST and structural validation, including duplicate
   arguments, duplicate locations, and empty location lists.
2. Compare names case-sensitively in ASCII.
3. Compare the complete argument-name set, type references and wrappers, and
   the presence and value of defaults.
4. Compare argument and location sets without depending on source order.
5. Treat string and block-string defaults as equal only when their decoded
   values are equal.
6. Ignore descriptions, comments, source paths, and spans.
7. Treat repeatability, extra arguments, and extra or missing locations as
   semantic differences.

An equivalent definition is consumed as an idempotent assertion of the core
built-in. Equivalent assertions may appear in multiple sources. The composed
schema, generated descriptor, and introspection retain only the core built-in
definition.

Any semantic difference produces `SF2004` at the directive name with the
message `Built-in directive definition '@NAME' conflicts with GraphQL September
2025`. Unknown directives remain ordinary user-defined directives. Normalization
must not change introspection results, operation validation, runtime behavior,
or the schema-descriptor fingerprint. Source provenance still records the input
file itself.

## 6. Resolver assembly API

### 6.1 Public surface

ADR-0002's one-method-per-field resolver traits remain unchanged. Assembly uses
a schema-ordered type-state chain:

```rust,ignore
let resolvers = generated::Resolvers::<AppContext, Types>::builder()
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_vault(VaultFields)
    .build();

let schema = generated::ExecutableSchema::<AppContext, Types>::builder(resolvers)
    .build()?;
```

Each `with_NAME` consumes one stage and exposes only the next schema object.
Only the terminal stage exposes `build`; completed object methods never reappear
and there is no dynamic registration method.

Method names use the shared GraphQL-to-Rust word partition and snake-case rules.
Apply keyword and raw-identifier escaping to the stem before adding `with_`, so
the generator never emits an invalid token such as `with_r#type`. A collision
between transformed resolver method names produces `SF4006`; the `with_` prefix
prevents a collision with the terminal `build` method.

Objects are ordered by semantic schema source order after composition and
extensions. Query and Mutation roots and source-defined object types participate;
interfaces, unions, input objects, enums, scalars, and introspection objects do
not require resolver-group registration.

### 6.2 Stage and storage invariants

The generated stage is a hidden, must-use type parameterized by `C`, `T`, and a
const index. A stage owns the accumulated object-specific dispatch adapters and
moves them into the next stage. The final `Resolvers<C, T>` stores the closed
slice.

The following properties are compile-time invariants:

- a missing resolver cannot build;
- duplicate and wrong-order methods are unavailable;
- unknown objects cannot be registered dynamically;
- each method requires its exact `NameResolver<C, T>` trait;
- resolver expressions evaluate once, left to right;
- concrete resolvers are erased exactly once through object-specific adapters;
- final public types do not accumulate one generic parameter per object; and
- storage uses no `unsafe`, `MaybeUninit`, `Any` downcast, or string-keyed map.

Stage names are `#[doc(hidden)]` public only where generated module reachability
requires it. Stages are not `Clone`, and fields remain generated-module-private.
Implementations may be sharded in groups of at most 16 objects rather than
hidden in one large macro or string.

### 6.3 Required compile diagnostics

Compile-fail fixtures match stable fragments, not complete compiler text:

| Case | Required fragments |
| --- | --- |
| missing or early build | `no method named \`build\`` and `ResolverStage` |
| duplicate | `no method named \`with_NAME\`` and `ResolverStage` |
| wrong order | `no method named \`with_NAME\`` and `ResolverStage` |
| wrong trait | object name and the corresponding resolver trait |

Fixtures require a nonzero `cargo check`; a must-use warning alone is not enough.

### 6.4 ABI migration

This decision moved `pythia::GENERATED_ABI_VERSION` from 2 to 3. All generated
sources were regenerated together. ABI 2 sources fail the ABI constant check;
no deprecated `Resolvers::new` compatibility layer remains.

## 7. Opt-in domain enum binding

### 7.1 Configuration

Unlisted SDL enums remain generated, Pythia-owned Rust enums. A closed TOML file
may opt selected names into domain bindings:

```toml
version = 1

[bindings]
domain-enums = ["ProtocolKind", "TransactionStatus"]
```

Only top-level `version`, the `bindings` table, and its `domain-enums` key are
allowed. `version = 1` is required. `domain-enums` is an array of unique GraphQL
Name strings. Unknown, duplicate, or non-enum names produce `SF4012`; syntax,
shape, I/O, path, and version failures produce `SF4011` with bounded location
information.

The file is limited to 64 KiB and 1,024 entries. `--config` accepts a path
relative to the current directory only. Absolute paths, `..`, symlinks,
non-UTF-8 paths, and implicit directory searches are rejected. `generate` and
`check` share the same option parser.

Entries normalize to schema source order. The recorded path uses `/`, omits
absolute host data, and is accompanied by lowercase SHA-256 of the exact file
bytes. Comments and whitespace therefore affect the configuration digest and
are visible to drift checking.

### 7.2 Generated types

For a bound enum, generated code keeps the SDL-shaped wire enum and adds a
binding trait with fallible `try_from_graphql` and `try_to_graphql` methods.
`SchemaTypes` exposes the application-selected associated type constrained by
that trait. The application implements the binding in its own crate; the domain
crate need not depend on Pythia.

Generated argument and input types propagate `T::Enum` through nullable, list,
input-object, and one-of positions without inventing additional enum generics.
Defaults are validated as wire values and converted while constructing resolver
arguments. Output conversion uses the same closed helper path.

`EnumBindingError` owns a private source error. Its client-independent display is
always `GraphQL enum binding failed`; only `Error::source()` exposes the cause,
and neither `Debug` nor `Display` includes the source message.

### 7.3 Error and schema behavior

Binding conversion failures are internal field failures, not client input
errors. They use message `Internal server error`, code
`INTERNAL_SERVER_ERROR`, the normal field path, and normal nullable/non-null
bubbling. The source is available only to the existing observability hook.
There is no panic, unknown-name string fallback, or first-variant fallback.

Bindings change Rust mapping and the generated/configuration digest, but not
GraphQL introspection, schema descriptors, SDL schema digest, or enum values.
Generated wire enums remain exhaustive; migration review must reject wildcard
domain mappings that would hide a newly added SDL variant.

## 8. `pythia_axum` adapter contract

### 8.1 Dependency and ownership boundary

`pythia_axum` is a separate adapter for Axum 0.8, Tokio 1, Tower 0.5, and HTTP/1.1
or HTTP/2 bodies. It reuses `pythia_http` preflight, decoding, media negotiation,
status mapping, and encoding. It does not duplicate codec decisions.

The application owns socket binding, TLS, routing, graceful shutdown, header
timeouts, CORS, compression, and health endpoints. The adapter never interprets
`Forwarded` or `X-Forwarded-For`; proxy trust belongs to application hooks.

### 8.2 Request, context, and response hooks

The adapter exposes:

- `RequestContext`, with read-only metadata, decoded request, admitted method,
  and negotiated response media;
- `RequestHook`, returning `Result<(), PrepareError>`;
- `ContextFactory<C, S>`, returning `PreparedContext<C, S>`;
- `ResponseHook<S>`, receiving a read-only response view and returning
  `ResponseHeaders`; and
- closure blanket implementations using the adapter's boxed future type.

`RequestMetadata` owns admitted method, URI, headers, extensions, and optional
`ConnectInfo<SocketAddr>`. `PreparedContext` moves `C` into execution exactly
once and retains `S` for the response hook. Shared application state is cloned
explicitly, normally through `Arc`; both types are `Send + Sync + 'static`.

### 8.3 Errors, rejection, and headers

`PrepareError` distinguishes internal `HookError` from application `Rejection`.
`Rejection::new` accepts only status 400 through 599 and carries one
`GraphQlError`; invalid statuses return `InvalidRejectionStatus`. Rejections use
the negotiated GraphQL media and the shared encoder rather than an
application-owned codec path.

Internal hook failures expose only message `Internal server error` and code
`INTERNAL_SERVER_ERROR`. `HookError` displays `GraphQL adapter hook failed`;
`InvalidRejectionStatus` displays `GraphQL rejection status must be between 400
and 599`. Causes remain observable through the existing application
observability layer, never through client text.

Response hooks cannot mutate GraphQL data, errors, or status. They may append
typed headers, including repeated `Set-Cookie`, but cannot set transport-owned
`content-type`, `content-length`, connection, proxy-authentication, transfer,
trailer, or upgrade headers. A forbidden or invalid header becomes a masked 500.

### 8.4 Builder and Axum composition

`GraphQl::builder(schema)` returns a builder without a context factory. Both
builder stages support request hooks and bounded settings; only
`context_factory` advances to the buildable stage, which may add a response hook.
Calling a setting or hook method again replaces its prior value; no implicit
hook chain is created.

The settings are body limit plus body-read, request-hook, context, execution,
response-hook, and total request timeouts. Defaults are 1 MiB and 30 seconds for
each timeout. Zero body limit rejects body-requiring requests while leaving
bodyless preflight behavior intact; zero duration means immediate timeout.

`ConfiguredGraphQlHandler` is cheap to clone through shared ownership and exposes
one async `handle(Request) -> Response`. Routing remains application-owned. A
POST-only Axum route returns 405 for GET before Pythia; a route that admits GET
lets the shared codec accept Query and reject Mutation. The handler performs no
path routing.

### 8.5 Lifecycle and timeout precedence

One request proceeds through:

1. method, media, and content-type preflight;
2. one bounded body read;
3. GraphQL HTTP decode;
4. request hook;
5. context factory;
6. Pythia execution and limits;
7. response hook; and
8. response encoding followed by approved header append.

A rejection stops all later phases. Hook headers are appended only after
successful encoding. An encoding or response-hook failure discards proposed
headers and returns the negotiated masked 500. Body limits are enforced during
streaming, not after buffering an unbounded body.

Every phase observes the earlier of its own deadline and the total deadline
measured from handler entry. Timeout drops the polled future; the adapter creates
no detached task.

| Failure | Status/code |
| --- | --- |
| codec preflight/decode | shared ADR-0004 mapping |
| body limit | 413 / `PAYLOAD_TOO_LARGE` |
| body stream failure | 500 / `INTERNAL_SERVER_ERROR` |
| body timeout | 408 / `REQUEST_BODY_TIMEOUT` |
| hook/context rejection | application status 400-599 and application code |
| internal hook/context failure | 500 / `INTERNAL_SERVER_ERROR` |
| phase or total timeout | 504 / `REQUEST_TIMEOUT` |
| response hook or serialization failure | negotiated 500 / `INTERNAL_SERVER_ERROR` |

Timeout messages are `Request timed out` or `Request body timed out` as
appropriate. The adapter does not promise rollback of external application side
effects that completed before a timeout or response-hook failure.

## 9. Differential migration contract

Migrate a bounded operation set in this order:

1. freeze the canonical SDL snapshot;
2. add Pythia generation/check and consumer quality gates;
3. configure custom scalars and explicit domain enum bindings;
4. reproduce authentication, DataLoader, rate limiting, and cookie behavior in
   application-owned Axum hooks;
5. run identical operation, variable, and authentication fixtures through old
   and new engines;
6. compare structured results; and
7. switch routes only after shadow or canary evidence.

The comparator ignores JSON object key order, but preserves array order, error
order, and path segment order. It compares data, error path/location/code,
non-allowlisted messages, admitted volatile extension fields, HTTP status,
response media, and explicitly listed application headers.

Each allowlist entry has a case ID, exact JSON pointer, old and new expected
values, reason, owner, and one of `specification-fix`,
`intended-product-change`, or `regression`. Only the first two may pass the
migration gate, and only with nonempty reason and owner. Wildcards and automatic
snapshot updates are forbidden.

Whole `/body` replacement is forbidden. ADR-0014 permits exact scalar/missing/
object/array markers below `/body/data`, `/body/errors`, and
`/body/extensions`; array changes are represented per index. Every allowlist and
volatile pointer must be valid, used exactly once, and tied to an existing case.

Complexity numbers need not match across engines. Fixtures select operations
that fit both admitted limits and separate adversarial operations rejected by
both. Repository gates use the allowed corpus; they do not copy an external application source.

## 10. Distribution and versioning

Libraries and `cargo-pythia` come from one workspace version, release, or exact
40-character Git commit. Moving branches/tags and unrelated filesystem paths do
not satisfy reproducibility.

Generated metadata records generator semver, ABI 3, source manifest, schema
content digest, normalized configuration label, and configuration SHA-256 or
`none`. Digests use ordered path bytes, explicit big-endian lengths, exact file
bytes, and lowercase SHA-256. Absolute host paths are never emitted.

`cargo pythia check` fails on generator version, ABI, schema/configuration digest,
or checked-in source drift. Release notes and README maintain the runtime/ABI
compatibility table, and reproduction uses locked dependencies.

## 11. Compatibility boundaries

This profile does not guarantee:

- automatic import of async-graphql extensions or nonstandard directives;
- derive compatibility or arbitrary Rust-type auto-introspection;
- identical old/new error wording or field scheduling;
- every consumer-selected Clippy lint;
- numerically identical complexity systems;
- federation, subscription, multipart, or incremental delivery; or
- codec equivalence after application middleware rewrites a response.

The observed Apollo client accepted both modern and legacy media types, but each
product and network contract still requires direct verification.

## 12. Alternatives considered

### 12.1 Product-side SDL preprocessing

Rejected because generator input would differ from canonical schema and weaken
drift detection. Normalization belongs in the importer.

### 12.2 Positional constructor with generated lint allows

Rejected because it neither fixes ordering mistakes nor makes dozens of similar
arguments readable.

### 12.3 Declarative registration macro

Rejected because macro hygiene, hidden helper reachability, module paths, and
diagnostics are more complex than a generated type-state chain.

### 12.4 Runtime resolver map

Rejected because string-keyed registration moves completeness and ordering from
compile time to runtime.

### 12.5 Bind every enum to domain types

Rejected because many products want the generated wire enum and because domain
types may intentionally live in dependency-independent crates.

### 12.6 Arbitrary Rust type paths in configuration

Rejected because generated crates cannot reliably name consumer-private paths
without coupling ownership and module layout.

### 12.7 Put Axum directly in facade/core

Rejected because it would violate the transport-neutral dependency boundary.

### 12.8 Let hooks return raw Axum responses

Rejected because it would bypass the shared GraphQL codec, redaction, and media
contract.

## 13. Consequences

### Positive

- Large products receive compile-time-complete, readable resolver assembly.
- Standard SDL printers interoperate without weakening directive semantics.
- Domain enums are explicit and fallible rather than magical conversions.
- Axum integration has typed hooks and one shared HTTP codec.
- Structural migration evidence is reviewable and bounded.

### Negative

- ABI 3 required regenerating every checked-in consumer.
- Type-state staging increases generated source volume.
- Domain binding configuration and adapter hooks add concepts that must be
  documented and tested.
- Applications remain responsible for routing, proxy trust, middleware, and
  rollback of external side effects.

## 14. Work packages

### WP-V0: Fixture and consumer gate

Capture the representative schema/operations, build the isolated consumer, and
record check, Clippy, rustdoc, dependency, and size baselines.

### WP-V1: SDL import normalization

Implement the closed built-in table, equivalence checks, conflict diagnostics,
and idempotence/provenance tests.

### WP-V2: Resolver assembly and ABI 3

Generate the type-state chain, compile-fail fixtures, shards, ABI guard, and all
regenerated consumers.

### WP-V3: Enum binding

Implement closed configuration parsing, digest/provenance, binding traits,
recursive input/output conversion, and redacted failures.

### WP-V4: Axum adapter

Implement the typed builder, request/context/response hooks, rejection and header
policies, bounded lifecycle, timeouts, and real HTTP tests.

### WP-V5: Differential example

Build the closed comparator, exact allowlist schema, representative corpus, and
shadow/canary procedure.

### WP-V6: Release and handoff

Regenerate all consumers, complete MSRV/stable quality gates, package and
dependency checks, documentation, independent review, and fresh-consumer proof.

## 15. Acceptance matrix

### 15.1 Repository command gate

The final source must pass implementation rules, formatting, MSRV and stable
check/Clippy/rustdoc, all generated exact checks, adapter and differential
integration tests, package/dependency checks, and a fresh locked consumer build.

### 15.2 Exact behavior gate

| ID | Input | Expected result |
| --- | --- | --- |
| SDL-01 | exact built-in definition from Section 5 | generation succeeds and descriptors equal the definition-free case |
| SDL-02 | reordered equivalent arguments or locations | generation succeeds |
| SDL-03 | description or block-string spelling changes only | generation succeeds |
| SDL-04 | `skip.if: Boolean` | nonzero exit, `SF2004`, name span |
| SDL-05 | different `deprecated` default | nonzero exit, `SF2004` |
| RES-01 | all resolver objects supplied in source order | check, Clippy, and rustdoc pass; each expression evaluates once |
| RES-02 | early build, duplicate, or wrong order | compile failure with Section 6.3 fragments |
| RES-03 | wrong resolver implementation | compile failure names the object resolver trait |
| ENUM-01 | no binding configuration | ABI 3 succeeds with generated owned enums |
| ENUM-02 | all 65 enums bound | resolver and input leaves use `T::Enum` |
| ENUM-03 | invalid syntax or unknown key | nonzero exit, `SF4011`, existing output unchanged |
| ENUM-04 | duplicate, unknown, or non-enum name | nonzero exit, `SF4012`, existing output unchanged |
| ENUM-05 | input or output conversion failure | internal field error with path; source visible only to observability |
| ENUM-06 | configuration order changes | schema descriptors, normalized configuration digest, and drift remain stable |
| AXUM-01 | POST Query and Mutation | shared codec preserves status, media, and body semantics |
| AXUM-02 | GET Query and Mutation through an admitted route | Query 200; Mutation 405 |
| AXUM-03 | application rejection 429 | negotiated GraphQL response with requested status and code |
| AXUM-04 | DataLoader context and cookie response state | loading works and repeated `Set-Cookie` headers are preserved |
| AXUM-05 | body limit and body-read timeout | 413 `PAYLOAD_TOO_LARGE`; 408 `REQUEST_BODY_TIMEOUT` |
| AXUM-06 | hook, context, execution, and total timeouts | future dropped; 504 `REQUEST_TIMEOUT`; no detached task |
| AXUM-07 | response-hook error or forbidden header | negotiated masked 500 |
| DIFF-01 | complete synthetic corpus | zero structural differences outside the exact allowlist |

### 15.3 Final review gate

Independent reviewers cover API/architecture and security/correctness/release.
Every finding is corrected and rerun before `Implementation: Implemented` is
recorded.

## 16. Security and operational notes

- Never trust proxy headers without application policy.
- Bound request bodies, all phase timeouts, diagnostic text, allowlists, and
  generated output.
- Keep error sources out of client responses and absolute paths out of generated
  metadata.
- Drop timed-out work and reap owned processes/tasks; do not detach them.
- Preserve application ownership of authentication, CORS, TLS, listeners,
  routing, external side-effect rollback, and operational telemetry.
- Treat generated crates, configuration digests, and package inventories as
  release artifacts rather than mutable runtime state.

## 17. Revisit triggers

Revisit this decision if GraphQL specification built-ins change, a supported
Rust version cannot express the type-state surface, resolver staging causes an
unacceptable compile-resource regression, a new adapter requires different
codec semantics, or a product needs a domain binding that the closed enum model
cannot represent.
