# Chigiri Architecture

Current compiler policy: the [ADR-0059 amendment](adr/0059-craftguild-standards-adoption.md#rust-195-unification-amendment-2026-11-07)
requires Rust 1.95.0 for all first-party builds, SDK generation, local checks
and CI, with declared minimum Rust 1.95. Earlier 1.85/Mock-only/stable lane
statements below retain historical scope and do not override this policy.

Historical execution records and implementation prompts are retained privately.
Public ADRs, this document and explicit test fixtures define the current
contract; historical Passed statements do not establish a current gate result.
See the [documentation index](README.md) and run the current validation gates.

Status: Accepted / developer usability implemented / production verification in progress
Subject Specification: [GraphQL September 2025](https://spec.graphql.org/September2025/)
HTTP Criteria: [GraphQL over HTTP Stage 2 draft @ `eae19ff`](https://github.com/graphql/graphql-over-http/blob/eae19ff930de8b1cde8511b16843175b0e537e66/spec/GraphQLOverHTTP.md)

2026-09-06 amendment: Section 23 and ADR-0026/0027 define the normative
contract for the next implementation phase. The earlier module-output, CLI,
configuration, ABI 5, and distribution completion conditions remain historical
implementation baselines; the newer ADRs govern the scope they change. A new
command is not part of the current binary contract until its implementation and
verification are complete.

2026-09-07 amendment: Section 24/ADR-0028 supersedes the former 350/400-line
sharding rules for generated output. The 400-line limit still applies to
handwritten files, while generated product output is divided into meaningful
units and remains subject to the existing quality and resource checks.

2026-09-08 addition: Section 25/ADR-0029 defines an isolated mock-CLI
experiment based on the fixed books fixture. It does not accept or implement a
change to production framework ownership or to the existing CLI/ABI.

2026-09-08 addition: Section 26/ADR-0030 is Accepted / Implemented. It replaces
the fixed-schema limitation with a mock-owned runtime source while preserving
the normal generated consumer-integrity contract and ABI 6.

2026-09-09 addition: Section 31/ADR-0035 (Accepted / Implemented) adds the
clap/ratatui TUI and GraphiQL. It changes only the stated GraphiQL asset,
routing, and external-pager contracts; all other boundaries remain in force.
Section 31 records the implementation status and residual conditions.

2026-09-23 amendment: Section 43 and
[ADR-0048](adr/0048-chigiri-brand-and-single-binary-cli.md) define the Accepted /
Implemented transition to Chigiri and a single public CLI binary. Earlier
product names remain only in classified history, external/user data, and named
rejection evidence. Local macOS acceptance and complete final-source
validation passed; other platforms and release distribution remain separate.

2026-09-24 addition: Section 45 and
[ADR-0050](adr/0050-portable-host-boundary-and-tiered-builds.md) define the
Accepted / Planned portable host boundary, Make interface, cross-build matrix,
and platform tiers. They do not claim implementation or platform evidence.

2026-10-17 amendment: Section 48 and
[ADR-0053](adr/0053-trait-only-resolvers-and-application-boundaries.md) define
Accepted / Implemented trait-only Rust integration and generated ABI 7.
Section 48 and its validation record distinguish passed focused local evidence
from historical projection/scaffold evidence and unexecuted qualification.

2026-10-23 amendment: Section 49 and
[ADR-0054](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md) define
typed Rust and Mock subscriptions at generated ABI 8. Local implementation and
focused acceptance are complete, including both whole-goal reviews; the dedicated
validation record identifies executed evidence and final source identities. The
former subscription rejection boundary is historical, not the current source behavior.

2026-10-24 amendment: Section 51 and
[ADR-0056](adr/0056-distributed-tracing-context-propagation.md) define opt-in
distributed parent adoption and SDK-owned execution-context propagation.
They preserve generated ABI 8 and default dependency boundaries. Existing
event-only observer evidence does not establish the execution guarantee. The
implemented contract has separate focused local evidence and independent
reviews, linked from Section 51.

## 1. Purpose

Chigiri uses GraphQL SDL as the single source of truth for the API and provides
a Rust framework for implementing that API with compile-time type safety.

```text
SDL
  -> CLI codegen
  -> generated resolver traits / input types / schema descriptor
  -> ordinary application resolver methods / explicit domain mapping
  -> generated Resolvers aggregation
  -> chigiri::Endpoint::builder (automatic safe baseline)
  -> immutable framework-neutral Endpoint
  -> optional HTTP framework bridge
  -> application-owned server
```

Applications supply SDL, ordinary resolver methods and their own server. The
Rust compiler checks that every SDL resolver has a compatible implementation.

The main conditions for success are:

- The SDL changes appear as a change to the generated Rust API.
- A missing resolver field or resolver group is a compile error.
- Applications can project domain types into GraphQL objects without first
  converting them to GraphQL-specific DTOs.
- Query and Mutation execute according to the September 2025 GraphQL
  specification.
- The core does not depend on an async runtime or HTTP server library.
- Middleware can be applied at Request, Operation, and Field scope.
- An application-owned server can call the framework-neutral endpoint and
  serve a working `/graphql` response derived from the SDL.

## 2. V1 scope

### In scope

- GraphQL SDL and type-system extension
- Query, Mutation, fragment, variable, directive
- Typed subscription sources and sequential fresh-context event execution
- built-in scalar, custom scalar, enum, input object, OneOf input object
- object, interface, union
- schema/executable validation
- variable, argument, input coercion
- introspection
- Query concurrent execution
- serial execution of mutation root fields
- Request / Operation / Field middleware
- GraphQL over HTTP by JSON POST and Query GET
- a framework-neutral endpoint API
- opt-in Hyper/Axum handler bridge
- Bounded WebSocket, distinct-connection SSE, and Apollo multipart subscriptions
- checked-in code generation and CI drift check

### Out of scope

- Legacy WebSocket protocols, shared-connection SSE, durable replay and receipts
- Apollo Federation / subgraph / router
- `@defer`, `@stream`, incremental response
- automatic persisted query (APQ)
- HTTP batch request
- multipart file upload
- GraphOS schema/usage reporting
- Sandbox or a general-purpose landing page (GraphiQL is the only optional UI,
  as defined by Section 31 and ADR-0035)
- TLS, HTTP/2, CORS, compression
- converting resolver panics into GraphQL errors

The parser, validator, generator and dedicated subscription executor support
subscription operations under Section 49/ADR-0054. Finite Query/Mutation entry
points do not silently turn into streams: applications use the explicit
subscription/streaming integration and continue to own their server lifecycle.

## 3. Component and dependency boundaries

The workspace is divided into the following crates:

| Crate | Responsibility | Normal dependencies |
| --- | --- | --- |
| `chigiri_core` | source/AST handling, SDL composition, validation, coercion, execution, middleware, request-deadline SPI, and the canonical `Value` type | none (standard library only) |
| `chigiri_host` | portable logical paths plus retained filesystem, process, interruption, writer, and native build-publication capabilities for developer tooling | `rustix`, `serde`, `sha2`, and target-scoped signal adapters |
| `chigiri` | endpoint builder, generated-ABI re-exports, and official component assembly | `chigiri_http`, `chigiri_observability`, `chigiri_operation`, `chigiri_policy`, `chigiri_rate`, `chigiri_rate_memory`, `chigiri_runtime`, and optional `chigiri_graphiql` |
| `chigiri_runtime` | schema construction, document caching, execution, and the private generated ABI | `chigiri_core`, `sha2` |
| `chigiri_codegen` | generator library, canonical Rust renderer, structured diagnostics, doctor operations, and configuration/provenance | `chigiri_core`, `chigiri_host`, `semver`, `serde`, `serde_json`, `sha2`, `toml_edit` |
| `chigiri_mock` | reusable experimental Mock operations, runtime, project management, and same-executable writer service | `clap`, `luaur`, formatter/parser dependencies, `chigiri`, `chigiri_axum`, `chigiri_http`, `chigiri_host`, `serde`, `tokio` |
| `chigiri_cli` | the sole `chigiri` product binary and its closed Rust/Mock namespace grammar | `clap`, `chigiri_codegen`, `chigiri_mock` |
| `chigiri_http` | server-library-neutral codec, body, deadline, hook, and lifecycle SPI contracts | `chigiri_core`, `chigiri_runtime`, `serde_json` |
| `chigiri_hyper` | application-owned request/body/timer/response bridge for Hyper services | `bytes`, `http-body-util`, `hyper`, `chigiri`, `chigiri_http`, `tokio` |
| `chigiri_axum` | application-owned Axum router and handler bridge | `axum`, `http-body-util`, `chigiri`, `chigiri_http`, `tokio` |
| `chigiri_graphiql` | pinned GraphiQL JavaScript, CSS, worker, font, and notice assets for an explicit feature; the endpoint view owns HTTP publication | none (standard library only) |
| `chigiri_operation` | persisted-operation catalog, APQ, and operation-source resolution | `chigiri_http`, `chigiri_runtime`, `sha2` |
| `chigiri_rate` | quota keys, policies, decisions, and object-safe provider SPI | none (standard library only) |
| `chigiri_rate_memory` | four bounded official in-memory providers and a monotonic clock | `atomic-waker`, `chigiri_rate` |
| `chigiri_policy` | operation-cost policy, two-tier rate limiting, and provider timeout/failure/observer composition | `chigiri_http`, `chigiri_rate`, `chigiri_runtime` |
| `chigiri_observability` | bounded observer, error/request ID | `blake3`, `crossbeam-channel`, `getrandom`, `chigiri_runtime` |
| `chigiri_tracing` | opt-in tracing observer | `chigiri_runtime`, `tracing` |
| `chigiri_opentelemetry` | opt-in OpenTelemetry observer | `opentelemetry`, `opentelemetry-semantic-conventions`, `chigiri_runtime` |
| `chigiri_testkit` | endpoint/provider conformance, operation-catalog impact, and runtime-readiness harness | `futures-util`, `chigiri`, `chigiri_http`, `chigiri_operation`, `chigiri_policy`, `chigiri_rate`, `serde_json` |
| `chigiri_books` | end-to-end example with an application-owned server | `bytes`, `clap`, `http-body-util`, `hyper`, `hyper-util`, `chigiri`, `chigiri_http`, `chigiri_hyper`, `chigiri_observability`, optional `chigiri_operation`/`serde`/`serde_json`/`sha2`, `chigiri_policy`, `tokio` |
| `chigiri_axum_adoption` | migration example covering authentication, DataLoader, cookies, and rate limiting | `axum`, `chigiri`, `chigiri_axum`, `chigiri_http`, `chigiri_testkit`, `serde_json`, `tokio` |

`chigiri_rate`, `chigiri_rate_memory`, and the dependency relationships shown
for them are implemented under ADR-0022/0023. `chigiri_policy` remains the
composition layer for Stage A/B admission, timeouts, failure policy, observers,
and operation cost.

The rules:

- `chigiri_core` forbids unsafe code with `#![forbid(unsafe_code)]`.
- The core public API does not expose Tokio, Hyper, Tower, Serde, or
  `serde_json::Value`.
- Generated resolver-trait signatures do not expose codegen or runtime
  implementation details.
- `chigiri::Endpoint` is the only façade that owns the private transport
  pipeline built on the HTTP lifecycle core. The bridges only convert framework
  request, body, timer, and response types to and from `GraphQlRequest` and
  `GraphQlResponse`.
- Chigiri's public API does not own listeners, connections, signals, or
  shutdown/drain behavior. The application owns routing. Section 31/ADR-0035
  adds only an optional route-building helper.
- Cargo features select dependencies across these crate boundaries; they do not
  collapse the workspace into one monolithic crate.

Rust baseline is Edition 2024, MSRV 1.85. CI runs both MSRV and current stable.

All implementations follow the [implementation rules](implementation-rules.md).
Handwritten `.rs` and `.sh` files, including tests, examples, and generators,
must remain at or below 400 physical lines. Canonical generated output follows
ADR-0028's semantic-unit layout. Under ADR-0043, all first-party documentation,
comments, rustdoc, diagnostics, examples, and generator-owned prose are written
in English. Run `./scripts/check-implementation-rules.sh` before work starts,
after each subtask, after `cargo fmt`, and immediately before handoff.

## 4. Build-time flow: SDL and code generation

### 4.1 SDL discovery and composition

The official CLI is manifest-driven:

```text
chigiri rust codegen
chigiri rust check
```

Doctor uses the same schema and configuration pipeline:

```text
chigiri rust doctor
```

After schema, configuration, and drift checks succeed, `doctor` runs
`cargo check --locked --message-format=json`. It preserves the original rustc
diagnostics and adds only high-confidence Chigiri guidance. ADR-0021 is the
normative source for structured diagnostics, the Cargo trust boundary, exit
codes 0/1/2, and resource limits.

Consumers that use domain-enum bindings pass the same explicit configuration to
both commands:

```text
chigiri rust --manifest chigiri.toml codegen
chigiri rust --manifest chigiri.toml check
```

The configuration path must be an explicit, secure relative path from the
current directory; the CLI never discovers it implicitly. The closed TOML
grammar is bounded to 64 KiB and 1,024 bindings. Path, I/O, TOML syntax, and
shape failures use `SF4011`; binding-name semantic failures use `SF4012`.
Diagnostics include a source span.

There is no Cargo alias or plugin executable. The `chigiri` binary calls the
codegen library directly.

`--schema-dir` discovers `.graphql` and `.gql` files. It normalizes relative
paths with `/` separators and sorts them lexicographically before parsing and
composition. Discovery does not follow symlinks and rejects duplicate canonical
paths. Limits are 256 KiB per source, 16 MiB total, 1,024 sources, directory
depth 64, and 65,536 directory entries. Metadata checks and bounded reads reject
oversized inputs before allocating their declared size.

Composition proceeds as follows:

1. Lex and parse each source under a distinct source ID.
2. Preflight public-AST names, numeric lexemes, nonempty extensions, and return
   type nesting.
3. Compose schema, type, and directive definitions and extensions into one
   semantic schema. Collect definitions before resolving references, apply
   extensions, stable-reorder members, directives, and origins into
   document/source order, rebuild the indexes in that order, and then validate.
4. When no explicit schema definition exists, use the specification's default
   root names: `Query`, `Mutation`, and `Subscription`.
5. Compare source definitions of `skip`, `include`, `deprecated`,
   `specifiedBy`, and `oneOf` with the specified built-ins. Normalize identical
   definitions into one entry; reject signature, location, default, or
   repeatability differences with `SF2004`.
6. Run all type-system validation rules.
7. Validate the subscription root and generate its complete source/event group,
   retaining ordinary field resolution when that root is a nested output.
8. Generate Rust source only after validation succeeds.

Failures report the path, one-based line and column, stable diagnostic code,
source line, and caret, and leave any existing generated output unchanged.

### 4.2 Deterministic generation

The former generated 350/400-line sharding baseline is superseded by Section 24
and ADR-0028. Resource limits, type completeness, deterministic output, and the
400-line limit for handwritten generator code remain mandatory.

- The generator does not depend on `syn` or `quote`; it uses a dedicated
  canonical Rust writer. Complete output is formatted with the pinned Rust
  1.85.0 rustfmt and an explicit temporary configuration, without inheriting
  settings from the current directory.
- Source and semantic ordering determine output. The same input produces a
  byte-identical file inventory and byte-identical files.
- Generation writes and flushes a complete candidate in a temporary directory,
  validates it, and then replaces the output directory as one process-level
  transaction. Detectable failures restore either the complete old directory
  or the complete new directory. V1 does not claim crash-atomic visibility if
  the process terminates between directory renames.
- `check` renders into memory or a temporary directory, compares the complete
  file inventory and bytes, exits nonzero on missing, extra, or changed files,
  and prints the exact regenerate command.
- Generated `mod.rs` contains the generated ABI 8 const guard, private
  `__chigiri` module, and application-facing re-exports. `__chigiri/mod.rs`
  records generator semver, the input-source manifest, schema-content SHA-256,
  and the binding configuration label and SHA-256 (or `none`). The schema digest
  hashes normalized relative-path length/path/content length/content tuples in
  discovery order, and the same digest is embedded in `SchemaDigest`. Private
  source and module indexes use the semantic-unit shards defined by ADR-0028;
  every generator-owned header and comment is English.
- A mismatch between generated and façade ABI versions produces an explicit
  const-evaluation compile error.
- Only root re-exports are application-facing. `__chigiri` and its private
  semantic-unit shards are not public API or public rustdoc.
- ADR-0028's semantic-unit inventory, ownership markers, formatting, Clippy,
  rustdoc, compile, exact-output, and resource checks replace line-count
  sharding for canonical generated files. Handwritten generator and renderer
  files remain subject to the 400-line limit.
- Before rendering, each GraphQL name is limited to 255 bytes and a conservative
  64 MiB upper estimate is computed from source bytes, schema members and
  directives, and recursively expanded const values. This prevents short source
  tokens from expanding into unbounded repeated names or descriptors. Rendered
  output is additionally bounded to 100,000 items, 65,536 final files, and
  64 MiB total; rustfmt batches stop at 512 items or 2 MiB.
- The generator never minifies output to evade a limit. Capacity failures use a
  stable `SF4xxx` codegen diagnostic.

### 4.3 Rust naming

- GraphQL type names become Rust `UpperCamelCase`.
- Field, argument, and input-field names become `snake_case`.
- Enum values become `UpperCamelCase`.
- Rust keywords use raw identifiers where Rust permits them, such as `r#type`.
  Names that cannot be raw identifiers (`self`, `Self`, `super`, `crate`, and
  `_`) use the deterministic ADR-0026 escape names: for type/variant
  positions, `__ChigiriNameSelf` and `__ChigiriNameUnderscore`; for field-like
  snake-case positions, the corresponding `__chigiri_name_*` form.
- If two GraphQL names map to the same escaped Rust identifier, generation
  fails instead of adding an automatic suffix.
- SDL descriptions become doc comments with control characters safely escaped.
  User-authored description text is preserved rather than translated.
- GraphQL deprecation/directive metadata is left in the static descriptor.
- Fixed headers, private-bridge descriptions, and safety/invariant comments
  owned by the generator are written in English.

## 5. Generated public API

Consider this SDL:

```graphql
scalar DateTime

type Query {
  user(id: ID!): User
}

type Mutation {
  renameUser(input: RenameUserInput!): User!
}

type User {
  id: ID!
  name: String!
  updatedAt: DateTime!
}

input RenameUserInput {
  id: ID!
  name: String!
}
```

### 5.1 Domain type mapping

Generated `SchemaTypes` associated types bind GraphQL objects and custom
scalars to application-owned Rust types.

```rust
pub trait SchemaTypes: Send + Sync + 'static {
    type User: Send + Sync + 'static;
    type DateTime: chigiri::Scalar;
}
```

Example of the application side:

```rust
struct Types;

impl generated::SchemaTypes for Types {
    type User = std::sync::Arc<domain::User>;
    type DateTime = domain::DateTime;
}
```

The framework does not generate output-object DTOs. The application chooses
whether an associated type is an owned domain value, `Arc<DomainType>`, or
another suitable representation.

### 5.2 Resolver traits

The root type and the object type are each generated with a resolver trait.

```rust
pub struct QueryUserArgs {
    pub id: chigiri::Id,
}

pub trait QueryResolver<C, T, E>: Send + Sync + 'static
where
    C: Send + Sync + 'static,
    T: SchemaTypes,
    E: std::error::Error + Send + Sync + 'static,
{
    fn user<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, C>,
        args: QueryUserArgs,
    ) -> impl std::future::Future<
        Output = Result<Option<T::User>, E>,
    > + Send + 'a;
}

pub trait UserResolver<C, T, E>: Send + Sync + 'static
where
    C: Send + Sync + 'static,
    T: SchemaTypes,
    E: std::error::Error + Send + Sync + 'static,
{
    fn id<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, C>,
        parent: &'a T::User,
    ) -> impl std::future::Future<
        Output = Result<chigiri::Id, E>,
    > + Send + 'a;
}
```

In a Rust trait implementation, the corresponding method may be written as an
`async fn`. The generated private module converts the RPITIT resolver into
`Pin<Box<dyn Future + Send>>` and builds an object-safe executor dispatcher.

A field with no arguments has no generated `args` parameter. An object-field
resolver receives `parent: &T::ObjectType`; an ordinary Query/Mutation root
resolver has no parent parameter. If that root object also appears in a nested position
as a field output, interface implementor, or union member, the generator does
not create another trait/group. It adds
`parent: Option<&T::RootType>` to the same root resolver, passing `None` for the
root operation and `Some(parent)` for a nested occurrence.

A Subscription root has separate `subscribe_<field>` and
`resolve_<field>_event` roles. If it also appears nested, its ordinary
single-value method receives `&T::SubscriptionType`, not an optional parent
and not a stream. Section 49 and ADR-0054 define these complete ABI 8 roles.

Every field remains required. Methods may appear in any order in an `impl`.
`SchemaTypes` binds domain values without an associated application error or
output-object DTO. Ordinary Rust calculations and `?` conversions stay in the
application API layer.

### 5.3 Resolver aggregation

Generated ABI 8 preserves linear typestate assembly. It requires a complete resolver
trait for every root and object, registered in schema order with one error type:

```rust
let resolvers = generated::Resolvers::<AppContext, Types>::builder(map_app_error)
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_user(UserFields)
    .build();
```

Each hidden `ResolverStage<C, T, E, const INDEX: usize>` exposes only the next
`with_NAME` method; only the final stage exposes `build`. The mapper fixes one
`E: Error + Send + Sync + 'static` across all groups. It is a synchronous
`Fn(E) -> FieldError + Send + Sync + 'static`, shared through `Arc` by private
adapters. Each selected resolver's returned error is mapped exactly once after
awaiting it. Framework conversion, middleware, and completion errors bypass it.

Adapters erase each complete group to `Arc<dyn ResolverGroupDispatch<C>>`.
The final public `Resolvers<C, T>` owns a boxed slice in schema order and exposes
no application error parameter. Explicit identity and exhaustive `Infallible`
mappers are supported. Disclosure uses an application-owned public allowlist;
internal sources, masking, IDs, paths, and normal execution policy remain intact.

Adding an SDL object extends assembly; adding a field extends its trait.
Regeneration compiles the SDK independently of the old application, then
consumer compilation exposes the missing work. Assembly uses neither `unsafe`,
`MaybeUninit`, runtime field maps, nor missing-field fallbacks. Projection
builders, setters, field stages, `ResolverFuture`, and `rust scaffold` are removed
without a second implementation mode.

### 5.4 Generated schema descriptor

Generated source does not reparse SDL at runtime. It contains a static
descriptor with:

- root operation type
- every type kind, name, and description;
- object and interface fields, arguments, and type references;
- interface implementations and union members;
- enum values;
- input fields, defaults, and OneOf flags;
- directive definitions, locations, and repeatability;
- type-system directive applications;
- deprecation reasons and custom-scalar `specifiedBy` URLs; and
- source coordinates for types and fields.

Here, "all types/directives" means source-defined elements. The generated
descriptor does not duplicate specified built-in scalars/directives or internal
introspection types. Directive applications on built-in scalar extensions are
preserved separately in an overlay descriptor, in source order within each
scalar. The runtime builder combines the generated descriptor, overlay, core
built-in descriptor, and internal introspection descriptor by name, without
reparsing SDL. `__Schema.types` contains generated/custom types and only those
built-in scalars reachable from field, argument, input-field,
generated/custom-directive argument, or built-in-directive argument type
references. It excludes unreferenced built-in scalars. All specified built-in
directives are present. Large member/type sequences preserve source order and
are split into fixed-width `DescriptorList` chunk tables that cannot encode
cycles.

`ExecutableSchema::builder` checks the descriptor schema fingerprint separately
from the resolver ABI fingerprint, which includes the binding-configuration
digest. This is an ABI invariant check between generated wrappers and the
dispatch table, not untrusted-input validation or resolver-completeness
checking.

## 6. GraphQL/Rust type mapping

### 6.1 Built-in scalar

| GraphQL | Rust |
|---|---|
| `Int` | `i32` |
| `Float` | `f64` |
| `String` | `String` |
| `Boolean` | `bool` |
| `ID` | `chigiri::Id` |

`ID` input accepts string and integer literals or JSON numbers and passes a
canonical string to the resolver. `Int` values outside the signed 32-bit range
produce coercion errors. `Float` output rejects NaN and infinity.

### 6.2 Output nullability/list

- `T!` -> `T`
- `T` -> `Option<T>`
- `[T!]!` -> `Vec<T>`
- `[T]!` -> `Vec<Option<T>>`
- `[T!]` -> `Option<Vec<T>>`
- `[T]` -> `Option<Vec<Option<T>>>`

Apply this conversion recursively at every level of list nesting.

### 6.3 Input null/undefined/default

GraphQL input distinguishes omission from explicit null.

```rust
pub enum Input<T> {
    Undefined,
    Null,
    Value(T),
}
```

Arguments and input-object fields use these presence representations:

| SDL | generated Rust |
|---|---|
| `x: T!` | `T` |
| `x: T! = default` | `T` |
| `x: T` | `Input<T>` |
| `x: T = default` | `Option<T>` |

A nullable list item cannot be omitted, so it uses `Option<T>`. Defaults are
applied before resolver invocation. At a nullable position with a default,
`None` represents explicit null or a null default; omission has already been
replaced by the default.

### 6.4 Input object / enum / OneOf

- An unbound GraphQL enum generates a Rust enum; unknown values fail coercion.
- An input object generates a Rust struct and applies the presence and
  nullability rules above to each field.
- Only input types that directly or transitively contain a custom scalar use a
  `T: SchemaTypes` parameter.
- An `@oneOf` input object generates a single-variant-choice Rust enum rather
  than a struct of nullable fields.

For a bound enum, the generator still emits the wire enum and connects it to an
application domain type through a `SchemaTypes` associated type and the
schema-local `NameBinding` trait. Only generated types that transitively contain
a bound leaf—directly, in a list, input object, recursive input, or OneOf—are
parameterized by `T: SchemaTypes`. Literals, variables, list items, input fields,
OneOf payloads, and output all use the same wire conversion. A conversion
failure becomes a masked field error whose source is `EnumBindingError`; it
never becomes a client message, panic, or unknown-string fallback.

```rust
pub enum UserUniqueCondition {
    Id(chigiri::Id),
    Username(String),
}
```

### 6.5 Interface / union

Interface and union results use generated enums that carry a concrete object
and its runtime type identity together.

```rust
pub enum SearchResult<T: SchemaTypes> {
    User(T::User),
    Organization(T::Organization),
}
```

An interface does not generate its own resolver group. Concrete-object
resolvers implement every object field, including fields declared by an
interface. The executor resolves `__typename` from the enum variant.

## 7. Core language/schema pipeline

### 7.1 Source and diagnostics

- The source text is UTF-8 `String`.
- A source span is `(source_id, start_byte, end_byte)`.
- The line-start table is computed once from the source and reused for
  diagnostic and response locations.
- One-based columns count Unicode scalar values. Spans remain byte offsets, and
  a tab counts as one source character.
- Byte-oriented inputs such as file contents are converted to validated UTF-8
  at the source-ingestion boundary; invalid encodings never enter the source
  registry.
- GraphQL names use the specified ASCII grammar; string values allow Unicode.
- The error code range is fixed as follows:
  - `SF1xxx`: lexer/parser
  - `SF2xxx`: schema composition/type-system validation
  - `SF3xxx`: executable validation/coercion
  - `SF4xxx`: codegen
  - `SF5xxx`: execution/runtime
  - `SF6xxx`: HTTP endpoint/bridge

### 7.2 Lexer/parser

The handwritten lexer handles punctuation, spreads, names, integers, floats,
strings, block strings, comments, the BOM, and ignored commas. It validates
number grammar, control characters, Unicode escapes, and surrogate pairs as
required by the specification.

The SDL and executable-document parsers are recursive-descent parsers over a
shared token stream. The common document parser recognizes both definition
kinds. The SDL composition entry point rejects executable definitions, and the
execution entry point rejects type-system definitions, both with stable
diagnostics. Parser recursion depth is measured explicitly so that exceeding
the configured limit returns an error before the process can overflow its
stack; parser panics are never used as parse errors.

The AST is public and mutable to support source preservation, tests, and
tooling. The schema-composition entry point therefore revalidates parser
invariants before making the semantic clone. Invalid names, integer or float
lexemes, empty extensions, and `TypeRef` or `Value` trees that bypass
`max_parser_nesting` are rejected with `SF2048` or `SF2049`. Only callers that
explicitly select `ExecutionLimits::unbounded()` remove this defensive limit.

### 7.3 Schema composition and validation

The semantic schema combines `BTreeMap`/`BTreeSet` for lookup with `Vec` for
source-order preservation. It does not use `IndexMap`, preserving the core's
standard-library-only dependency boundary.

The implementation follows the normative rules in Section 3 of the September
2025 GraphQL specification. In particular, the following checks run as
separate validation passes:

- root operations and root type kinds;
- uniqueness of type, field, argument, input, enum, and directive names;
- input and output type positions;
- interface implementations and covariant returns;
- union-member constraints;
- non-null singular input cycles and input-default cycles;
- OneOf field and default constraints;
- directive-definition cycles, locations, and repeatability;
- extension target kinds and duplicate members; and
- reserved `__` names.

Each pass stops when the diagnostic limit is reached. Successful composition
indexes interface, union, and object candidates once; executable validation
does not rescan the complete schema.

### 7.4 Executable validation/coercion

After parsing and before execution, the implementation applies the September
2025 Section 5 rules for fragment cycles, field selection, arguments, variable
definitions and uses, directives, and OneOf literals.

Document-validation passes have a fixed order: public-AST invariants; definitions and indices; fragments; fields, directives, and field merging; variable definitions and uses; literal and variable positions; and the complexity summary. Diagnostics follow validation-phase order and then rule-subphase order while preserving AST or schema definition order. Once the diagnostic limit is reached, the current literal walk and all later passes stop; a partially validated document is never published.

The pipeline order is fixed:

```text
parse
  -> document validation
  -> operation selection
  -> raw variable coercion
  -> operation middleware
  -> field collection / argument coercion / execution
```

If a document contains multiple operations and no operation name is supplied,
selection fails with a request error. An unknown supplied name also fails. The
document cache stores validated documents only; variable coercion is performed
for every request and is never cached. Schema indices provide direct lookup for
fields, directives, and values. Input-object requiredness and default plans are
built lazily for each referenced type and reused within the same validation or
coercion.

Custom scalars delegate input and output conversion to `Scalar::parse` and
`Scalar::serialize`. Serialization may produce null, a primitive, an object, or
a list. ADR-0015 applies output depth, list-item, request-wide node, and HTTP
byte limits to that complete JSON representation. Built-in scalar result
coercion is unchanged. Constant literals are passed to the registered parser
before a validated document enters the cache; at the strict entry point, an
unregistered constant-literal callback is a validation error. For literals
containing variables, only variable positions are validated statically, and
the parser runs during request-time coercion.
The core validates custom executable directives for definition, location, and
arguments, while middleware supplies their application semantics. Only the
built-in `@skip` and `@include` directives are evaluated directly by the
executor.

## 8. Execution model

### 8.1 Runtime-neutral async

The core does not spawn tasks or own timers, sockets, or runtime semaphores.
Resolvers return `Future + Send`; the executor polls them from its own future.

Ordinary selection sets use a bounded poll set built from `std::future::poll_fn`
and pinned boxed field futures. This requires no thread-level parallelism; it
interleaves I/O progress on the same task.

### 8.2 Query/Mutation ordering

- Query roots and nested selection sets execute concurrently within the bound.
- Mutation root fields execute serially in collected source order.
- Nested selections below each Mutation root field execute concurrently.
- Response object fields are emitted in collected-field order, independent of
  completion order.

`max_concurrent_fields` limits the number of in-flight fields in a selection
set. The v1 core does not provide a global request semaphore because that
policy depends on the runtime and application.

### 8.3 Value completion/error propagation

Resolver output is converted to the following owned, erased representation.
The application object is stored as `Any + Send + Sync + 'static` and borrowed
only while the executor invokes nested resolvers. A list item retains its own
serialization error until nullable-item completion.

```rust
pub struct ResolvedObject {
    type_name: &'static str,
    value: Box<dyn Any + Send + Sync>,
}

pub enum ResolvedValue {
    Null,
    Leaf(Value),
    Object(ResolvedObject),
    List(Vec<FieldResult<ResolvedValue>>),
}
```

The executor performs result coercion and completion according to the field's
`TypeRef`. An error at a nullable position produces null there; an error at a
non-null position propagates to the nearest nullable parent. Once propagation
makes sibling results irrelevant, unfinished sibling futures are dropped
cooperatively.

Production permits at most 1,000 items in a completed field list and 100,000
field/list output nodes for the request. Exceeding either bound produces a
masked internal error that reveals neither the threshold nor resolver values.
JSON uses a bounded writer, and an encoded response over 8 MiB is replaced by a
masked 500 response. Applications should apply pagination and ADR-0011 cost
policy before a resolver creates a large `Vec`. Custom-scalar object members and
list items participate in the request node budget, and every embedded list uses
the item bound. Custom-scalar output depth is 128; validation and destruction
are iterative to avoid a recursive stack overflow.

An internal `FieldError` created by resolution, completion, or introspection
retains its source and enters the façade's common diagnostic path. That path
assigns the error ID, notifies `ErrorObserver`, and converts the failure into a
client-safe `GraphQlError`.

For the ordinary framework, a nested non-null error does not roll back an
application side effect already performed by a Mutation resolver. The
application owns such transactions. Mock request atomicity is the separate
contract in ADR-0039.

## 9. Request pipeline and middleware

### 9.1 Application context

`ExecutableSchema<C>` receives one `C: Send + Sync + 'static` for each request.
Core execution shares it immutably as `&C`. The application uses `C` to own
resources such as a database pool, DataLoader, authenticated principal, and
request-scoped cache.

The HTTP endpoint's context factory creates the request context. Ordinary
functions or captured-service closures use `PreparedContext::execution_only`;
[compiled recipes](rust-integration.md#per-request-context) show the request
lifetime. Share pools through `Arc` while allocating caches and session state
for each request. The advanced `PreparedContext<C, S>` path preserves response
hook state, preparation ordering, deadlines, and drop/cancellation.
Preparation retains `PrepareError::Reject` and `PrepareError::Internal` and
never enters the resolver mapper. A creation
failure is an HTTP/application error before GraphQL execution. The framework
bridge does not create context; it only passes neutral request metadata.

### 9.2 Three middleware phases

```rust
pub trait Plugin<C>: Send + Sync + 'static {
    fn request<'a>(
        &'a self,
        ctx: RequestContext<'a, C>,
        next: RequestNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse>;

    fn operation<'a>(
        &'a self,
        ctx: OperationContext<'a, C>,
        next: OperationNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse>;

    fn field<'a>(
        &'a self,
        ctx: FieldContext<'a, C>,
        next: FieldNext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}
```

All three methods default to `next.run(ctx)`.

- Request middleware runs after HTTP decode and context construction but before
  document parsing or cache lookup. It can inspect the raw document, operation
  name, variables, and extensions.
- Operation middleware runs after validation, operation selection, and variable
  coercion. It can inspect operation kind and name, coerced variables, and the
  complexity summary.
- Field middleware runs after argument coercion and immediately before resolver
  dispatch. It can inspect the schema coordinate, response path, parent type,
  coerced arguments, and selection metadata.

Plugins enter in registration order and exit in reverse order. `Next` is a
consuming, non-`Clone` type that can be invoked once. Request and Operation
middleware may short-circuit with a response; Field middleware may
short-circuit with a value or error.

Plugin instances must be `Sync` because fields may run concurrently.
Applications choose their own mutex or atomic strategy for mutable plugin
state and must not hold a blocking lock guard across `.await`. A panic is an
application failure, not a GraphQL error; core does not use `catch_unwind`.

## 10. Public runtime API

The principal request, response, and error types are:

```rust
pub struct GraphQlRequest {
    pub query: String,
    pub operation_name: Option<String>,
    pub variables: Object,
    pub extensions: Object,
}

pub struct GraphQlResponse {
    pub data: Option<Value>,
    pub errors: Vec<GraphQlError>,
    pub extensions: Object,
}

pub struct GraphQlError {
    pub message: String,
    pub locations: Vec<Location>,
    pub path: Vec<PathSegment>,
    pub extensions: Object,
}
```

`data: None` means that a request error prevented execution from starting.
`data: Some(Value::Null)` means that execution started and produced GraphQL
`null`.

Core `Value::Int` stores values through `i128` directly. Larger integers remain
lossless as `Value::BigInt` containing the canonical decimal lexeme. Expansion
of canonical integers from JSON exponent notation is charged to a shared budget
across the entire nested request value. The budget is 1 MiB, matching the HTTP
request-body limit, and an overrun is a resource error. Expected-type coercion
enforces the GraphQL `Int` `i32` range, `Float` exactness rules, and canonical
string conversion for `ID`. An ordered `Object` preserves first-insertion order
for responses and maintains a lookup index, so an untrusted JSON object does not
need to be rebuilt for lookup.

`FieldError` has two constructor families:

- `FieldError::public(message).code(code).extension(...)` exposes the selected
  message and extensions to the client.
- `FieldError::internal(source)` exposes only `Internal server error` with
  `INTERNAL_SERVER_ERROR`, while retaining the original error for middleware
  logging.

The framework reserves these `extensions.code` values:

- `GRAPHQL_PARSE_FAILED`
- `GRAPHQL_VALIDATION_FAILED`
- `BAD_USER_INPUT`
- `OPERATION_NOT_FOUND`
- `OPERATION_NOT_SUPPORTED`
- `BAD_REQUEST`
- `METHOD_NOT_ALLOWED`
- `UNSUPPORTED_MEDIA_TYPE`
- `NOT_ACCEPTABLE`
- `PAYLOAD_TOO_LARGE`
- `REQUEST_BODY_TIMEOUT`
- `REQUEST_TIMEOUT`
- `INTERNAL_SERVER_ERROR`

### Schema construction

```rust
let schema = generated::ExecutableSchema::<AppContext, Types>::builder(resolvers)
    .plugin(TracingPlugin)
    .limits(chigiri::ExecutionLimits::production())
    .introspection(chigiri::IntrospectionPolicy::Disabled)
    .document_cache(chigiri::DocumentCacheLimits::default())
    .build()?;

let response = schema.execute(request, app_context).await;
```

`ExecutableSchema` is an `Arc`-backed handle that is cheap to clone. V1 always
masks an internal error with the fixed public message and code; it does not
expose a configurable error-policy API for unsupported settings.

## 11. Document cache and limits

Each schema instance owns its document cache. Raw query text is the key and an
`Arc`-wrapped validated document is the value. Invalid documents, variables,
and responses are never cached.

The v1 cache uses a standard-library `Mutex` around a bounded map and insertion
queue. A hit clones the `Arc` while holding the lock and releases the lock before
execution. By default, the cache holds at most 1,024 entries and 16 MiB of query
key text; it evicts the oldest entries until both bounds are satisfied.

Production defaults:

| Limit | Default | Scope |
|---|---:|---|
| HTTP body | 1 MiB | HTTP endpoint |
| HTTP body read timeout | 30 s | HTTP endpoint + application/bridge `DeadlineFactory` |
| context factory timeout | 30 s | HTTP endpoint + application/bridge `DeadlineFactory` |
| GraphQL document | 256 KiB | direct execution and HTTP |
| parser nesting | 128 | parser and public AST-composition preflight |
| operation depth | 32 | validation |
| expanded fields | 1,000 | validation after fragment expansion |
| aliases | 100 | validation |
| variables | 100 | validation/coercion |
| directives | 100 | validation |
| diagnostics | 100 | each complete compiler phase |
| concurrent fields / selection set | 64 | executor |
| execution timeout | 30 s | HTTP endpoint + application/bridge `DeadlineFactory` |

GraphQL and endpoint limits share the public `ExecutionLimits` type. Tests and
internal tools may opt into `ExecutionLimits::unbounded()` explicitly; an
endpoint never selects it by default. Header timeouts, connection and in-flight
limits, and drain timeouts belong to the application server.

Introspection and schema suggestions (`Did you mean...`) are disabled by
`Endpoint::builder`'s automatic safe baseline. Operator tooling that requires
introspection must enable it through the explicit dangerous API with a stated
reason; Chigiri does not infer a development mode from environment variables.

## 12. GraphQL over HTTP

`chigiri_http` is server-library-neutral: it does not expose framework request,
response, or server types. It supplies the transport-neutral
`TransportRequest`, `RequestBody`, and `DeadlineFactory` codec/SPI contracts.
After application routing, `chigiri::Endpoint` owns the private request state. It
accepts only the case-sensitive method tokens `GET` and `POST`, evaluates
`Accept` and `Content-Type`, and controls body polling before execution.

### Supported request

This contract begins after the application forwards a route such as `/graphql`
to the endpoint. Chigiri does not match paths.

- `POST`
  - `Content-Type: application/json` is required.
  - The body is a JSON object containing `query`, `operationName`, `variables`,
    and `extensions` as applicable.
  - Query and Mutation are allowed.
- `GET`
  - The same fields are carried as URL query parameters.
  - `variables` and `extensions` are percent-decoded JSON objects.
  - Validation and operation selection run before the endpoint permits only a
    Query; a selected Mutation returns `405 Method Not Allowed`.

Batch arrays, `application/graphql` bodies, and multipart requests are not
accepted.

### Response negotiation/status

`Accept` negotiation follows RFC 9110 specificity and quality rules, including
media ranges, UTF-8 charset parameters, and parameter count. A range with
incompatible parameters is not a match. At equal quality,
`application/graphql-response+json` takes precedence over legacy
`application/json`.

| Condition | HTTP status |
|---|---:|
| malformed URI encoding / JSON syntax | 400 |
| request body read timeout | 408 |
| GraphQL document parse error + `graphql-response+json` | 400 |
| request shape / GraphQL validation / operation selection / variable coercion error + `graphql-response+json` | 422 |
| GraphQL Request Error + legacy `application/json` | 200 |
| field error / partial data after execution starts | 200 |
| request body limit exceeded | 413 |
| GET Mutation | 405 |
| unsupported Content-Type | 415 |
| no supported response media type in `Accept` | 406 |
| execution timeout | 504 |
| context factory timeout | 504 |
| internal or response-serialization failure | 500 |

The response body is always the JSON representation of core
`GraphQlResponse`; the endpoint does not replace an execution error with a
separate HTTP error format. For `application/graphql-response+json`, the pinned
draft's Request Error classification applies: document parse errors use 400,
and errors in a well-formed request's shape, validation, operation selection, or
variable coercion use 422. For compatibility, GraphQL Request Errors encoded as
legacy `application/json` use 200.

Field Errors and partial data use 200. The pinned draft is inconsistent between
its status-code recommendation of a non-200 response and its later 200 example;
v1 chooses 200 for interoperability.

Context and execution timeout responses omit `data` and expose the client-safe
message `Request timed out` with `extensions.code = REQUEST_TIMEOUT`. A body
read timeout likewise omits `data` and exposes `Request body timed out` with
`extensions.code = REQUEST_BODY_TIMEOUT`.

## Framework-neutral GraphQL endpoint and opt-in bridge

`chigiri::Endpoint<C>` is the application-facing entry point. It owns a
`chigiri::spi::ExecutableSchema<C>`, the context factory, the automatic safe
baseline, one typed component graph, direct execution, and handler-level
request processing. Every neutral request runs through the same schema and
policy.

`ExecutableSchema` supplies schema identity, the semantic schema, resolver
dispatch, custom scalars, and catalog revision. Plugins, samplers, observers,
ID factories, field instrumentation, limits, cache, and introspection policy
are assembled only in the final endpoint graph. Generated-resolver input and a
prebuilt schema receive the same baseline and override semantics.

```rust
let endpoint = chigiri::Endpoint::builder(resolvers)
    .context_factory(AppContextFactory::new(state))
    .rate_limit(transport_rate)
    .inject(|components| {
        components.rate_limit().stage_b(operation_rate);
        components.observability().error_observer(observer);
    })
    .build()?;

let response = endpoint
    .handle(transport_request, application_body, &application_deadlines)
    .await;
```

`TransportRequest` has no body. `Endpoint::handle` does not poll `RequestBody`
until method/media preflight and transport admission have succeeded. It applies
the body limit and source, context, execution, and overall deadlines. The
standard-library-only `chigiri_core::DeadlineFactory` supplies only a runtime
timer future; HTTP and direct entry points carry the same request factory into
the operation context. Endpoint phases and rate providers poll each target
future in the request task and drop only that target on timeout. ADR-0004 is the
normative trait contract.

Deadlines provide cooperative future cancellation; they cannot preempt a
synchronous block inside a provider method or `poll`. The testkit watchdog
rejects blocking third-party providers. A backend that requires hard isolation
must use its own bounded worker.

The lower-level endpoint builder and handler were removed from `chigiri_http`;
the façade's private module now owns 14-phase assembly and execution state.
`chigiri_http` exposes only codecs, hooks, body/deadline SPI, and lifecycle
contracts, with no constructor that bypasses the façade. `chigiri_hyper` and
`chigiri_axum` are opt-in bridges that convert framework request heads, bodies,
timers, and responses to the Chigiri-owned types above. They do not reimplement
the GraphQL codec, phase order, or policy. The Hyper bridge connects to an
application-owned service; the Axum bridge connects to a handler in an
application-owned `Router`.

The application owns the runtime, socket bind/accept, `/graphql` route, TLS,
HTTP version, header timeout, connection and in-flight limits, signals,
shutdown/drain, and health/readiness routes. Chigiri provides no `chigiri_server`,
listener builder, or standalone `serve(address, shutdown)` API. The books
example implements those responsibilities as application code and remains a
consumer-integration example rather than a framework distribution.

Hyper, Axum, Tokio, and Tower dependencies stay inside their corresponding
bridges. No framework type appears in `chigiri_core`, the `chigiri` façade, or a
neutral public contract. Consumers that do not use a bridge may implement
`RequestBody` and `DeadlineFactory` directly.

## 14. Introspection

September 2025 § 4.2 provides the normative introspection schema as an internal
descriptor. Appendix D in the same edition retains an older form with nullable
`includeDeprecated`; v1 follows § 4.2 and interprets all five occurrences as
`Boolean! = false`.

- `__typename` is available regardless of the introspection policy.
- `__schema` and `__type` are only available if the policy is Enabled.
- It implements `__Type.isOneOf`, `specifiedByURL`, deprecated arguments and
  input fields, and their `includeDeprecated` controls.
- Introspection fields are resolved internally and never call application
  resolvers.
- When introspection is disabled, access produces a field error and unknown-
  field suggestions do not reveal introspection names.

## 15. Versioning/compatibility

- The crate version is a single workspace version.
- Generated code uses an independent integer ABI. Any incompatible change to
  the generated private surface raises that ABI.
- The current generated ABI is 7. There is no reader, adapter, or shim for an
  earlier ABI.
- Library crates and `chigiri` derive from one workspace version and use
  either the same exact release or the same full 40-character Git revision.
- A difference in generator semver, ABI, or schema/config provenance causes
  `chigiri rust check` to fail.
- Public resolver traits, `GraphQlRequest`/`GraphQlResponse`, and middleware
  contexts follow semver.
- Generated private modules and source layout are outside semver, while a
  generated/runtime ABI mismatch fails clearly at compile time.
- SDL breaking-change classification and a schema registry are outside v1;
  generated diffs and application/client CI handle them.

## 16. Implementation completion criteria

- `chigiri_core` has no normal dependencies.
- `./scripts/check-implementation-rules.sh` passes, every handwritten `.rs` and
  `.sh` file is at most 400 physical lines, and generated files satisfy the
  ownership and semantic-unit layout checks from ADR-0028.
- First-party comments and rustdoc explain the reasons for GraphQL invariants,
  security decisions, and concurrency rules.
- Every applicable row in the September 2025 Query/Mutation conformance matrix
  links to a test.
- Invalid SDL never changes generated output.
- A fixture missing an SDL field or resolver group fails to compile.
- Unit tests verify concurrent Query siblings, serial Mutation root fields,
  response ordering, and null bubbling.
- Tests cover Request, Operation, and Field middleware ordering,
  short-circuiting, and error conversion.
- HTTP integration tests cover POST Query, POST Mutation, GET Query, GET
  Mutation rejection, and media/status mapping.
- Following only the README procedure, the books example can be edited,
  generated, compiled, served by an application-owned server, and queried with
  `curl`.
- The neutral endpoint and the Hyper and Axum bridges pass the same lifecycle
  suite. Bridge public APIs expose no listener, route, connection, or shutdown
  ownership.
- `cargo test --workspace` and `chigiri rust check` pass on the MSRV and current
  stable Rust.
- The 70-object, 750-field, 19-input, 65-enum, and 5-scalar fixture compiles
  without warnings on the MSRV and current stable Rust.
- ADR-0007 acceptance IDs `SDL-01` through `DIFF-01` trace to concrete tests,
  and the Axum timeout test verifies that the future is dropped.
- Generated source contains only ABI 8, and the descriptor/header schema
  digest, config digest, and configuration-aware regeneration command are
  reproducible.
- The Axum adoption corpus with authentication, a request-scoped loader, rate
  limiting, and cookies has no differences outside its allowlist.
- Two independent reviews report no Critical, High, or Medium findings, and a
  fresh clone of the final HEAD reproduces from the committed lockfile.

## 17. Implemented existing-product profile

The ADR-0007 existing-product profile was implemented as generated ABI 3.
Evidence for WP-V0 through WP-V6, acceptance-ID-to-test mappings, independent
reviews, and fresh-clone results is recorded in
Beads review `lab01-c43.9`.

## 18. Accepted production extension (Implementation in progress)

The independent crate and endpoint/bridge boundaries accepted by the ADR-0004
amendment and ADR-0008 through ADR-0013 WP-P0 have been implemented.
Performance, load, long-duration fuzzing, the current-HEAD candidate, and final
package/fresh-clone evidence remain incomplete, so the ADR implementation and
Gates P-D and P-E are not complete. An independent review of commit
`3120c870536ca82c8537beef9b49963af50f29ba` closed every resource and privacy
finding from the broad security review received on 2026-08-28, leaving zero
Critical, High, Medium, or Low findings. Gate P-B and Gate P-C therefore return
to Passed, and the security-review condition for Gate P-E is satisfied.

| crate | Planned responsibility | Normal dependency boundary |
| --- | --- | --- |
| `chigiri_operation` | registry, catalog, APQ, source mode | façade/HTTP contracts; no client tooling |
| `chigiri_policy` | checked cost, quota key, rate SPI/in-memory provider | core metadata/façade; no runtime or Redis dependency |
| `chigiri_observability` | ID factory, sampling, bounded exporter, metric model | stable core events; no backend types |
| `chigiri_tracing` | Opt-in bridge from stable events to `tracing`. | Only observability users. |
| `chigiri_opentelemetry` | Opt-in bridge from stable events to OpenTelemetry. | Only observability users. |
| `chigiri_testkit` | endpoint/bridge/provider/comparator/redaction/drop conformance | consumer dev-dependency |

Runtime-invariant identities, observer views, and error ID values live in
`chigiri_runtime`, preserving the standard-library-only language/execution core.
`chigiri_runtime` computes SHA-256; `chigiri_http` owns JSON/HTTP envelopes and
the neutral SPI; and the façade-private transport pipeline beneath
`chigiri::Endpoint` owns the neutral lifecycle, request-phase deadlines, and
body processing. Bridges only convert body, timer, and framework types. The
application owns sockets, routing, connections, and shutdown. Bridges compose
production features through the `chigiri_http` SPI rather than depending
directly on crates such as `chigiri_operation`.

`chigiri::Endpoint::builder` produces an immutable, automatically safe baseline
for error and request IDs; bounded execution, cache, body, response, and
deadline handling; CSRF; and visibility. Stage A/B rate limiting is
unconfigured by default because it requires application identity. Applications
configure it explicitly with the official algorithm builder or typed provider
injection. A provider timeout drops only that provider future and is not reused
as the endpoint-wide transport or execution timeout. It is delivered as a
provider failure to the configured fail-open or fail-closed policy. The books
example runs the same endpoint and official rate builder directly and through
the Hyper and Axum bridges.

HTTP and direct entry points generate the request ID and make the sampling
decision exactly once, then pass the same `RequestTelemetry`, including an
external trace ID when present, to source resolution, policy, and resolvers.
The OpenTelemetry bridge uses a bounded active-request map to preserve the
explicit parent hierarchy across the request root, operation resolution,
execution, providers, and opt-in field spans, and releases every context at the
request terminal. Request-ID generation failure does not alter request
semantics; the observer receives an ID-less `REQUEST_ID_GENERATION_FAILED`
event with a fixed sampling decision. Error-ID generation failure likewise
does not change masking or request semantics and does not retry ID generation;
it emits an ID-less `ERROR_ID_GENERATION_FAILED` event.

The standard request-ID factory remains an uninitialized plan in
`Endpoint::builder`. The final `build()`, called after worker-process forks and
before accepting requests, obtains one 256-bit seed from the operating-system
random source. Clones share that seed and an `AtomicU64` counter. For each
request, the domain separator and counter are passed to a BLAKE3 keyed hash,
whose first 128 bits become 32 lowercase hexadecimal characters. Counter
exhaustion or use from a process other than the creator causes factory failure
rather than a duplicate ID. An `AtomicBool` one-shot returns the build probe's
first ID exactly once, without a request-wide mutex or a lock held while
calling a lower-level factory. The standard error-ID factory remains
independent of this failure-sensitive hot path and obtains fresh operating-
system randomness for every error.

`Endpoint::in_flight_requests()` reports only requests whose futures are being
polled and returns to zero when those futures complete or are dropped.
Application-owned readiness combines the application's request count with this
value; the application owns connection draining after a shutdown signal.

### 18.1 Production request order

HTTP execution has 14 fixed phases: preflight, transport admission, bounded
body, decode, source resolution, decoded hook, context, Request plugin,
parse/cache, validation/selection/coercion, Operation plugin/admission,
Field/resolver, observer/hook, and bounded encoding/header append. Direct
execution begins at the Request plugin. Timeouts drop the phase future or
stream on the same task and never leave a detached task behind.

### 18.2 Generated ABI transition history

WP-P1 made a private ABI change that moved the canonical schema digest from the
generated descriptor into immutable `SchemaInfo`. At that time the runtime,
generator, golden files, examples, and large consumer were updated together to
ABI 4. ADR-0018 later introduced sealed projection input and the private
directory layout, moving the system to ABI 5 and deleting checked-in ABI 4
output and its runtime shim.

### 18.3 Production completion boundary

The `production-ready` label may be added only after WP-P0 through WP-P8,
Gates P-A through P-E, all 53 acceptance IDs, three review tracks,
fuzz/load/package/SBOM/provenance evidence, and a fresh clone of the final HEAD
are complete. Sections 9 through 11 of `implementation-handoff.md` and Sections
2.8 through 2.9 of ADR-0013 are the normative sources for the predeclared
thresholds and trace plan.

## 19. Implemented custom scalar output governance

ADR-0015 removed the earlier SCALAR-OUTPUT-01 restriction. A custom scalar's
`Scalar::serialize` result may now be null, a primitive, an object, or a list.
Built-in scalar and enum completion retain their dedicated paths; only custom
scalars enter engine-owned custom completion.

`ExecutionLimits::production()` sets custom-scalar output depth to 128. Object
member values and list items, excluding the custom root, are reserved
atomically against the request-wide output-node budget before materialization;
the field-item limit applies to every embedded list. The iterative validator
checks depth, list length, node count, finite floats, and canonical BigInts,
then commits the reservation only after the entire value passes. Invalid or
deep containers are dismantled with the shared iterative decomposition helper,
avoiding recursive `Drop`.

HTTP continues to enforce `response_byte_limit` through the bounded writer and
counts the exact escaped UTF-8 encoding. Direct execution does not add an
estimated byte limit. CSO-01 through CSO-15 and the generated direct and neutral
HTTP paths are mapped in `spec-conformance.md` Section 11; review evidence is
in Beads review `lab01-c43.7`.

## 20. Implemented developer usability program

The breaking migration in ADR-0016 through ADR-0019 reduced the
application-facing surface without weakening the production or security
contract. The implementation completed the ABI 5 and `Endpoint::builder`
transition, the 54 acceptance IDs, automated gates, Reviews A and B, and the
final-commit fresh-clone verification. Gate U-A through Gate U-E and the ADR
implementations are complete. Formal performance, load, and long-fuzz evidence
is incomplete, so Gate P-D/P-E remains Not Passed. This program is not
designated as production-ready.

```text
checked-in SDL
  -> chigiri rust codegen (generated ABI 5)
  -> complete resolver traits or typed projection plans
  -> generated Resolvers assembly
  -> chigiri::Endpoint::builder (automatic safe baseline)
       -> convenience methods and inject update one typed component graph
  -> immutable neutral Endpoint
  -> application-owned direct / custom / Hyper / Axum bridge
```

The `chigiri` package is the application façade; `chigiri_runtime` contains the
lower-level implementation. `Endpoint::builder` is the only application entry
point. Chigiri owns no standalone server, runtime, listener, route, or shutdown.
Hyper and Axum remain opt-in adapters.

The builder automatically applies production structural limits, deadlines,
masking, CSRF policy, and visibility defaults. Every change that weakens safety
is available only through a named dangerous API.
The builder explicitly reports rate limiting as unconfigured because it cannot
infer application identity; it never presents an unconfigured endpoint as
protected. Sugar methods and `inject` mutate the same component graph. The last
call wins for a single slot, while collections have explicit push, set, and
clear semantics.

When a built schema is used as input, it does not import a second component
graph. Collection `set` and `clear` operations therefore apply to the single
final graph. Configuration failures are returned as closed slot, origin
(`Baseline` or `User`), and violation enums rather than diagnostic strings;
they do not include configured values or reasons. Builder state remains
editable until `build()` creates the endpoint. A prefork application may edit
the builder in the parent, but must call `build()` in each worker before that
worker accepts requests.

Generated source checks in the material required for compile-time completeness,
while private modules and `doc(hidden)` keep implementation detail out of the
ordinary rustdoc and IDE surface.
For non-root objects, a linear typestate projection plan can resolve simple
fields with synchronous closures. Roots, fields with arguments, asynchronous or
fallible fields, and deeper control use the complete resolver contract. Both
forms turn a missing field into a compile failure and have no runtime registry
or null fallback.
ADR-0020 does not add a third resolution form; it provides a stdout-only
scaffold for entering explicit projection closures.
The scaffold does not derive, reflect over, or implicitly discover domain
members, and it never merges application source automatically.

The official in-memory rate package implements token-bucket, fixed-window,
sliding-window-counter, and queue-free leaky-bucket algorithms. ADR-0011 still
governs Stage A/B order, the atomic provider SPI, bounded key memory,
deadline/drop behavior, and failure policy. Third-party providers implement the
same SPI and testkit contract. Concurrency remains within the existing
execution boundary; permits are not mixed into the time-quota SPI.

There is no old preset guide, duplicate builder, ABI 4 reader, or legacy rate
policy compatibility shim. `implementation-handoff.md` Section 12 and
`spec-conformance.md` Section 12 are normative; the automated surface check is
`scripts/check-usability-contract.sh`.

## 21. Implemented DX and rate boundary hardening

ADR-0020 through ADR-0024 preserve the execution semantics of generated ABI 5
and ADR-0016 through ADR-0019 while implementing these tooling, crate, and
evidence boundaries:

```text
explicit projection contract
  <- stdout-only chigiri rust scaffold

schema/config/drift
  -> chigiri rust doctor
  -> cargo check --locked JSON
  -> original rustc diagnostic + bounded Chigiri help

chigiri_rate (std-only SPI)
  <- chigiri_rate_memory (four official in-memory providers)
  <- chigiri_policy (Stage A/B composition)
  <- chigiri façade
```

ADR-0011/0019 continue to govern the `RateLimiter` atomic decision, the four
integer algorithms, Stage A/B ordering, the default fail-closed 500 ms policy,
last-write-wins behavior within a stage, and bounded memory. After the crate
split, a third-party provider needs only `chigiri_rate` as a normal dependency.
Official backends are in-memory; an application or provider crate implements
the same SPI for a distributed backend.

The large-consumer gate uses at least 70 objects and 750 fields and combines
scaffold, positive and negative doctor cases, MSRV/stable, public rustdoc,
third-party SPI, and all four official providers in one revision's evidence.
It does not replace request-performance, load/soak, or long-fuzz evidence, so
Gate P-D/P-E remains Not Passed until its manual evidence is complete.

`implementation-handoff.md` Section 13 is normative for implementation order
and Gate D-A through D-E; `spec-conformance.md` Section 13 is normative for the
69 acceptance IDs. Gate D-A through D-E are complete, including Reviews A/B,
clean-package verification, and isolated fresh-clone reproduction.

## 22. Correlated operation audit identity

ADR-0025 correlates Audit-mode security events with the existing request
lifecycle. The operation-source resolver constructs a borrowed
`RequestIdentity` from the same `RequestEnvelope`'s `RequestTelemetry` and
passes it to `OperationAuditEvent`. An entry point without telemetry uses an
empty identity; it neither generates a replacement ID nor fails the request.

```text
HTTP/direct admission
  -> RequestTelemetry(request ID / trace ID / sampling)
  -> RequestEnvelope
  -> operation source resolution
       -> registry lookup
       -> OperationAuditEvent(identity(None), digest, revision, outcome)
       -> application-owned audit sink
       -> provider/sink failure with the same identity -> ErrorObserver
  -> parse / validate / operation selection
```

Operation identity is not yet known during source resolution, so
`identity.operation()` is always `None`. Document correlation uses the existing
`document_digest()`. An unverified operation name is never used as a temporary
identity, and a later operation key does not flow backward into the source
phase.

Sampling controls telemetry-export volume, not security-audit admission. Even
under `SamplingDecision::Drop`, the audit sink receives each hit, miss, and
provider-unavailable resolution attempt exactly once. Provider and sink failures
remain fail-open under ADR-0010 and notify `ErrorObserver` exactly once with the
same request and trace identity.

Audit events expose request and trace IDs to the sink only as read-only
correlation values; they add no raw document, variables, principal, IP,
session, or provider source. `Debug` redacts identity values, and metrics never
use request or trace IDs as labels. An application copies only the bounded IDs
it needs when retaining an event beyond its borrow lifetime.

Acceptance evidence runs a real Axum endpoint with both a failing registry and
a failing audit sink. It verifies byte-exact normal GraphQL data after
fail-open, and confirms that the HTTP status/body/headers and standard
`LifecycleExporter::ErrorRecord` expose no request/trace ID, raw document, or
provider/sink source. With no telemetry, both success and provider/sink failure
retain an empty identity and default record, with audit/error notification
exactly once.

This change affects only a server-side public event. It does not change HTTP
responses, catalog/APQ/Safelist/IdOnly behavior, the crate graph, schema,
generator, or generated ABI 5. `implementation-handoff.md` Section 14 governs
Gate OA-A through OA-D; `spec-conformance.md` Section 14 governs AUDIT-ID-01
through AUDIT-ID-08. All eight IDs, independent Reviews A/B, package checks, and
isolated fresh-clone reproduction are complete, so Gate OA-A through OA-D is
Passed. Formal performance, load, and long-fuzz evidence is outside this gate;
Gate P-D/P-E remains Not Passed.

## 23. Sibling generator program (Accepted / Implemented)

The normative sources are
[ADR-0026](adr/0026-generated-contract-crate-and-manifest-cli.md) and
[ADR-0027](adr/0027-lightweight-validation-and-generator-build-identity.md).
They supersede earlier contracts where the decisions conflict.

- The workflow separates SDL, the dedicated generated crate,
  application-local types/resolvers/projections, and the single endpoint. The
  generated crate does not depend on the domain or application crate, and real
  consumer compilation proves completeness.
- Opt-in domain enum associations and bidirectional conversions live on the
  application-local `Types`, avoiding the orphan rule for an external trait and
  external domain type.
- `chigiri/config-v2` is the only manifest format. The `chigiri rust`
  namespace provides `init`, `codegen`, `update`, `check`, `doctor`,
  `scaffold`, and help/version. CLI and environment overrides cannot replace
  schema, package, output, runtime, or consumer authority.
- The output is a marker-owned standalone crate. `codegen` creates an absent
  destination exclusively; `update` verifies a private candidate before
  renaming it into place. The transaction does not claim crash-atomic or
  zero-downtime visibility. It records commit, rollback, and cleanup failures,
  excludes concurrent operations through the parent-directory lock, and keeps
  recovery archives of old bytes until final cleanup. Recovery placement,
  cache rebasing, and conservative duplicate detection when a path is absent
  follow ADR-0026 Section 5.
- Generated ABI 6 replaces the former module, CLI, configuration, and ABI
  reader. Binding fingerprints, manifest/input provenance, and
  generator/runtime identity remain separate classifications.
- Generator build identity appears in CLI version output and generated
  `generator()`/JSON records. It is not a full-byte source attestation, does not
  restamp output after every commit, and supplies no signature or
  authentication. `check` preserves the recorded identity and re-renders with
  the current renderer for a complete byte comparison, without requiring a
  post-commit restamp.
- Runtime sources may use path or Git selection through normal Cargo
  mechanisms. Local path development is sufficient for product verification.
  Core remains standard-library-only, and runtime-neutral GraphQL, HTTP,
  observability, rate, and audit security contracts remain intact.
- Distribution-specific package, SSH, and clean-clone checks are reported
  separately from product completion. Manual publication and tagging remain
  future work; formal performance, load, and long-fuzz evidence remains a
  separate product gate. ADR-0027 Section 5 controls the narrow dependency-
  policy exceptions for local SDK paths and ordinary generated SDKs. Formatting,
  implementation-rule checks, and full-product tests are consolidated into
  common validation. Dependency checks still cover runtime and third-party
  dependencies plus unregistered or handwritten crates.

Linux/macOS CLI I/O capability and runtime-target portability are separate
concerns. The 400-line handwritten limit, semantic-unit layout for generated
output, English rustdoc, pinned formatter, and bounded input, output, and child
processes remain mandatory.
The CLI broadcasts SIGTERM and SIGINT through explicit tool and transaction
cancellation scopes; the library never registers process signals implicitly.
Human and `chigiri/diagnostics-v2` JSON output share one bounded diagnostic model
that redacts raw source at entry. ADR-0026 Section 4 defines text, item, report,
and truncation limits. `doctor` combines the static exact check with real
Cargo-metadata validation of the workspace, package, SDK, runtime, and locked
consumer selection. Bounds and confidence for real locations, feature
expressions, Git-source adapters, and source annotations follow ADR-0026
Section 5. It retains original rustc diagnostics rather than trying to infer
every complex Rust relationship. Handoff Section 15 defines ordering,
Conformance Section 15 defines acceptance IDs, and the sibling-generator goal
prompt defines the work package. Application consumer migrations are separate.

## 24. Semantic generated layout amendment (Accepted / Implemented)

[ADR-0028](adr/0028-semantic-generated-layout-and-handwritten-line-budget.md)
adds requirements to Section 23. Canonical generated Rust is no longer subject
to the 350/400-line rule, while handwritten generators, renderers, consumers,
tests, and scripts retain the 400-line maximum. Generated types are organized
by owner, root resolver contracts by Query or Mutation role, and projections by
object. Internal dispatch, coercion, and descriptors remain separate
responsibilities; aggregate indices do not justify one monolithic schema file.

Kind, role, and coordinate determine file paths; line-count packing and
sequential shards are removed. Unrelated type changes do not alter independent
model or public-trait paths or bytes. Global metadata and indices are reported
separately from real dependency changes. Public API, ABI 6, SchemaDigest,
binding fingerprint, execution behavior, and internal structures required for
type safety and bounded tables remain intact.

Only a registered manifest/output root with a verified marker and complete
inventory receives the generated exemption. A self-declared header, arbitrary
marker, matching directory name, or manual edit cannot bypass ownership
checks. Exact generation, formatting, Clippy, rustdoc, compilation, resource
limits, transactions, and recovery remain mandatory. Handoff Section 16 and
Conformance Section 16 define SG-LAYOUT-01 through SG-LAYOUT-08 and Gate SG-F.

## 25. Experimental Luau blog mock CLI (Accepted / Implemented)

The normative source is
[ADR-0029](adr/0029-experimental-luau-mock-cli.md). `chigiri_mock` is a private,
experimental Lab01 application. Its original executable supplied `init` and
`serve`; those operations now run through `chigiri mock`. The original `init`
generated a fixed editable TOML/Luau
project, and `serve` ran User/Post CRUD and relations over a volatile store.
This was a user-managed scripted execution project, not canonical generated
Rust.

- A fixed SDL generated an ordinary typed contract crate and connected its
  resolvers to a Luau worker. GraphQL execution used one endpoint and an
  existing bridge; the ordinary runtime gained no arbitrary-SDL registry.
- The Mock application owned listeners, CORS, loopback admission, workers, and
  shutdown. This preserved the Section 13/ADR-0004 contract that the framework
  and bridges do not own the server lifecycle.
- Luau, compiler, and typechecker dependencies remain confined to the
  experimental package; they are not introduced into the production façade,
  runtime, core, or existing CLI. The core's standard-library-only boundary,
  ABI 6, complete resolvers, projections, and GraphQL/HTTP semantics do not
  change.
- The 2026-09-08 amendment permits MSRV 1.95 only for the mock package. Existing
  products and generated crates remain on 1.85, and the generation formatter
  remains 1.85.0. Mock is a separately selected package in the same workspace
  and is excluded from the default members. Common validation checks existing
  products on 1.85, mock on 1.95.0, and the full workspace on stable, rejecting
  omission of mock or leakage of its dependencies into normal products.
- `Post.authorId` referenced a User and `User.posts` was derived; the two sides
  did not maintain independent relation state. Relation fields resolved from
  one read snapshot and did not eagerly expand cyclic models.
- Each Mutation root used a copied store and counter state and committed only
  after successful result validation. The initial contract did not provide
  request-wide rollback after a later root or HTTP failure.
- Experimental operation-line admission remained in the CLI and did not alter
  concurrent Query polling in the core. VM memory, Rust callback/store/queue
  bounds, and explicit shutdown complemented the deadline; a deadline alone
  was not treated as proof that native work stopped. Rust/Luau re-entry through
  `pcall` was limited to 16 active levels, with the seventeenth rejected before
  re-entry. A caught re-entry overflow stayed sticky and prevented commit.

General schema-to-IR generation, AI generation, general-purpose relations,
persistence, and a hostile multi-tenant sandbox were deferred. Handoff Section
17 and Conformance Section 17 define MOCK-01 through MOCK-16. Real VM and HTTP
tests, independent review, release measurement, and the 64-command local suite
passed. A follow-up review corrected `pcall` return handling and confirmed in
debug and release that re-entry is rejected, both models roll back, and the
server recovers. The resulting common suite contained 65 commands. Final-source
and unexecuted Linux/remote-CI evidence is in the
validation record.

WP-M0 confirmed through a focused VM unit test that luaur 0.1.8 is incompatible
with Rust 1.85. Validation item M0-01 records approval of a Mock-only Rust 1.95
exception. It does not authorize changing the VM, core dependency boundary, or
ordinary product MSRV.

## 26. Runtime schema Mock IR program

Status: Accepted / Implemented. The normative source is
[ADR-0030](adr/0030-runtime-schema-mock-ir-and-directives.md); the implementation
request is the goal prompt.
The Passed result in Section 25 and ADR-0029 is historical evidence for a fixed
blog schema, not proof of this program's generality.

The same Mock library reads multiple SDL files during `chigiri mock init` and
derives the common IR and Luau/TOML from their directives. During `serve`, it rebuilds
the IR from embedded SDL, validates every seed and script, and connects the
result to the existing `Endpoint`. Startup does not require the original SDL,
schema-specific Rust, Cargo, external Lua, or network access. The IR retains
models and payloads, complete type wrappers, operation plans, owner/inverse
relations, scalar representations, and constraints. TOML does not duplicate
those meanings, and `init` rejects invalid SDL relations.

At the ADR-0030 checkpoint, only explicit CRUD operations were automated.
Non-CRUD behavior used manual Luau, and `serve` failed before binding if any
entry remained unimplemented. ADR-0051 Section 46 supersedes that admission
rule: a contract-valid intentional-error implementation may now be served.
Seeds begin empty. AI inference, automatic seed synthesis, persistence, and
Subscription are out of scope. Rust's generic store handles 1:1, 1:N, M:N,
self, and nullable relations while enforcing references, RESTRICT behavior,
uniqueness, and all-model commits. Interface, union, and non-model payloads are
validated against concrete types; custom scalars use explicit JSON
representations and closed constraints.

Core, runtime, and the façade may accept an owned runtime schema. Rather than
fabricating static descriptor/type-name allocations or a false generated
fingerprint, the implementation distinguishes the schema source and owns it per
instance. This does not weaken the standard generated trait/typestate model,
single-endpoint graph, or HTTP, GraphQL, observer, and limit contracts.
The exception to the usual dynamic-registry prohibition is limited to this
validated Mock schema and field dispatch. It does not permit production
consumers to turn missing resolvers into runtime fallbacks.

Luau describes operation combinations; Rust owns typing, authorization,
consistency, transactions, and cancellation. ADR-0029 keeps the fresh VM,
snapshot, sticky-failure state, 16-level `pcall` limit, worker recovery, and Rust
resource accounting separate. Core continues to support Rust 1.85 and generated
ABI 6 without depending on Mock, while Mock requires Rust 1.95. The work follows
Handoff Section 18; admission follows Conformance Section 18 and ADR-0030
acceptance IDs RSM-01 through RSM-22. The verification record identifies the
fixtures that prove removal of the old fixed Mock dependency and preserves the
existing security guarantees.

WP-RSM1 connects parsing and binding of `experimental::OwnedSchema` to a
single `Endpoint` input. `SchemaInfo` reports the source kind and optional
generated ABI; owned inputs never claim generated-ABI provenance. A dedicated
startup/request coercion budget of 100,000 nodes and 16 MiB of logical bytes
stops default expansion and value copying. Generated ABI 6 completeness and
the existing limits remain intact. ADR-0030 Section 7.1 defines the concrete
API and accounting rules. All Gates G-A through G-E, acceptance IDs RSM-01
through RSM-22, the common 66-command suite, and independent Reviews A and B
are complete. Usage, two-domain, rename, offline, measurement, and unexecuted
CI evidence is recorded in the
verification record.

WP-RSM2 uses `mock/src/schema/vocabulary.graphql` as the sole source of SDL
vocabulary. Equivalent user definitions are retained; only missing definitions
are appended as the final logical source, and every source contributes to the
digest. When the vocabulary is fully supplied, its comment-only appendix is
still retained as a source. Helper types and directives use separate
namespaces. `SchemaGraph` resolves named, field, and input IDs; complete
wrappers; defaults; OneOf; abstract members; and scalar and field constraints.
If scalar and field constraints for one value have an empty intersection, the
schema is rejected. Constructing this type graph does not authorize execution
or establish model/CRUD meaning.

`MockIr` resolves models, keys, storage slots, owners and inverses, CRUD
mappings, and manual coordinates to IDs. Minimum and maximum inverse reference
counts are intersected per owner; a mapped output limit is never reused as the
original edge-count limit. A script's `mutation_root` marks eligibility as a
Mutation root, while write permission is checked separately at the actual call
site. `ValueValidator` borrows coerced inputs, storage slots and DTOs, and
custom scalars under one shared budget. During `MockIr` construction, existing
coercion expands defaults and validates input and storage constraints.
Directive-definition argument defaults count against the same budget. The
validator walks fields present in the value and required-field plans from the
IR rather than rescanning every type field for each sparse object. Integer
constraint intersections also verify the Int i32 range and the existence of an
integer value.

`store/` collects model registration order, seed-ID reservations and counters,
and owner-derived indexes in candidate state. Records are shared through
`Arc`, and only modified objects are copied. After complete consistency checks,
the prepared state is exchanged once, following a cancellation check. Seed
JSON uses borrowed `RawValue` and existing serde/HTTP numeric conversion, and
checks duplicate keys, depth, and capacity before expansion. `output/`
validates model copies against every slot/presence snapshot, DTO value
projections, and concrete abstract members. Every object uses the same
snapshot; readers and prepared values share the remaining budget and failure
state through result validation. `codec/` converts typed values using owned VM
values, a null sentinel, and host markers, applying the value budget before
copying and caching table-capacity walks. `luau/` generates IR-derived type
declarations, static CRUD/manual candidates, and the exact `ScriptSet`, then
checks standalone syntax and expected types. Complete-wrapper capacity is
checked before compiler startup, including escaped bytes in string keys.

`runtime/` validates actual root and nested call sites and connects them to the
common host/codec/store/result exchange under the cancellation lock. It retains
the `pcall` re-entry limit, cancellation, and the global allowlist. `service/`
connects a bounded worker queue and owned dispatch to CLI v1, and listens only
after all scripts have been validated and readiness is established. `init`
embeds multiple SDL files as logical sources and exclusively creates output
only after revalidating the rendered self-contained TOML. The new path was
verified by 41 CLI/HTTP cases, including the previous 30. Owned response
keys, paths, typenames, leaves, errors, directives, and introspection use the
shared byte budget from ADR-0030 Section 7.1. Root-type nested use is recorded
in the IR so the same coordinate receives a distinguishable nullable-parent
`ReadContext` or `WriteContext`. A process gate prevents the VM/checker's
adopted version from competing with common flags while preserving the existing
500 ms execution deadline and cancellation.

## 27. Mock model API / generated reference program

[ADR-0031](adr/0031-mock-model-operations-and-generated-reference.md) is the
implemented follow-up contract. Runtime registration and dispatch, argument
validation, Luau declarations, and reference output all consume the typed
definitions in `crates/mock/src/api/`. `api/shape.rs` defines closed predicate
and sort shapes; `api/context.rs` defines scope, null, model, and JSON
constructors; and `api/model_values.rs` centralizes the key, omission,
outer-null, and typename policies for Record, Create, and Update. Update's
outer-null type representation remains distinct from non-null storage
constraints, and nested DTO types are preserved. Reference `api_types`
represents these meanings as one shared stable-ID type graph bound to models
and slots.

Search builds one `QueryPlan` that separates storage constraints from operand
validation and shares its evaluator between `find` and `count`. Owner
connect/disconnect operations reuse the existing update candidate records,
indexes, and transaction. Query and nested operations retain a fixed snapshot;
Mutation roots retain candidate state, sticky failure, and the cancellation
commit fence.

Reference generation reuses the closed configuration envelope, schema, and
digest without invoking `serve` store or script validation. A common
`ReferenceDocument` renders text and JSON while sharing the type graph and
limiting construction and output to 16 MiB each. It explicitly reports
`readiness_checked = false`. TOML and API are v2 and the reference document is
v1; the old v1 flat-`where` form is not supported. Acceptance IDs MR-01 through
MR-27, Gates MR-A through MR-E, the common 71-command suite, and independent
Reviews A and B Passed locally. Implementation evidence, per-ID assertions,
measurements, and final source mappings are collected in the
ADR-0031 verification record. That MR
evidence is distinct from Section 26's historical Passed result. Remote
Linux/macOS CI remains unexecuted and is not counted as a successful MR gate.

## 28. Mock project / portable bundle program

[ADR-0032](adr/0032-mock-project-layout-and-portable-export.md) is the follow-up
contract to ADR-0031. It separates editable project v1, portable bundle v3,
API v2, and reference v2. Luau settings are evaluated once and converted into
owned settings, while the current IR is built from external SDL. Public types
and module paths are fixed by role and coordinate and never expose temporary IR
positions. Both input forms converge on the same type/module initialization and
seed-validated snapshot, sharing the existing executor and store. Project input
for reference generation reads only settings and SDL and does not establish
readiness.

`update` protects user edits through the ADR's B/U/N states and ownership
inventory, treating the complete layout as the rollback unit. It holds the
exclusive project-directory lock and the starting descriptors and bytes until
placement completes. Recovery uses a private transaction manifest and restores
all candidates and backups as a unit. Hard-link sharing between replacement
targets and separate inputs is rejected before mutation; non-moving adoption
remains allowed. Pre-commit restoration failures are distinguished from
post-commit cleanup failures, and unknown or modified files and recovery
materials are never recursively deleted. Temporary manifest/recovery bounds
and bounded retention of old and new generations follow ADR-0032 Section 7.3.

`export` renders the same validated snapshot into deterministic bundle v3 and
publishes it exclusively to a new file outside the project. Configuration,
initialization, and requests share the minimal global set, VM admission, sticky
failure, and existing capacity and deadline limits. Product ABI 6 and MSRV
1.85, and Mock MSRV 1.95.0, remain unchanged. All MP acceptance IDs and gates,
the fixes and re-reviews from independent Reviews A and B, the common
73-command suite, and the two-domain execution and measurement under an
external-network block are complete. Source evidence is collected in the
ADR-0032 verification record. The
2026-09-09 designer review also Passed with zero new findings at every
severity. Linux/remote CI remains a separate gate.

## 29. Agent-assisted Mock inference program

Status: Accepted / Implemented. The historical normative source is
[ADR-0033](adr/0033-agent-assisted-mock-inference.md). The feature reasoned only
about selected active resolver or JSONL files, presented a complete diff, and
installed byte-verified candidates only after explicit approval. Mock owned IR,
API, reference, and partial validation; a bounded backend trait owned request,
deadline, cancellation, and result semantics; the Codex adapter alone owned the
child process. Model and reasoning-effort overrides followed the same rules as
local execution. All batches, corrections, and isolated validation shared one
absolute deadline: 600 seconds by default, or an explicit value in the closed
1-to-86,400-second range.

`project/active.rs` built one owned settings, SDL, snapshot, and readiness view
for both normal commands and inference. Context generation exposed real
reference types and split a closed active selection into identified batches.
Partial validation reused ordinary module and relation-linked seed checks while
reporting total readiness separately. The internal
`chigiri-mock-validation/v1` protocol bounded input to 64 MiB, output to 32 MiB,
depth to 12, and tokens to 131,072, and compared schema, API, type, selection,
and request digests. The isolated worker did not reread configuration, SDL, or
source and did not treat a dropped native checker thread as recovered.

Proposals were published from private staging to a new directory with complete
original, candidate, instructions, and all-file inventories. Bounds included
an 8 MiB manifest, 32 MiB aggregate, 642 files, path sharing, and a distinct
1.5 MiB invalid-seed storage limit; that storage limit did not replace the
256 KiB existing-seed limit used during adoption. Reopening a proposal
verified closed shape, individual and aggregate sizes, digests, link counts,
descriptors, and inventory. Adoption compared the stored candidate against the
latest active state, revalidated dependencies and readiness, and committed
owned bytes under the cancellation-aware project lock only after final I/O
checks. AI adoption produced user-edited U state and did not advance static B.

The historical TUI separated selection, search, instructions, transmission,
complete diffs, rejection, retesting, and final approval. It preserved source
bytes, isolated bracketed paste and typeahead between stages, restored terminal
state, and supported non-TTY proposal saving. Two-domain and rename HTTP cases,
the 75-command common suite, Reviews A/B, and the explicitly authorized real
Codex acceptance passed locally. The
validation record distinguishes
those results from unexecuted remote CI. The earlier Passed result was not
reused as evidence for this acceptance. ADR-0044 later removed built-in
`infer`/`apply`; this section remains historical architecture.

Required stored non-null model edges were also analyzed for cycles that cannot
be completed under finite Mock values. `init` and `check` reported a
representative cycle. Nullable/list edges and explicit `@relation` storage were
excluded because they do not establish the same unsatisfiable value cycle.

## 30. Generated Luau formatting program

Status: Accepted / Implemented. The normative source is
[ADR-0034](adr/0034-generated-luau-formatting.md). It added a formatting
boundary to generated and candidate Luau without invoking an external CLI or
discovering user configuration. The fixed pipeline was: render the complete
source, format the complete syntax tree, run existing type and semantic checks,
bind digest and management identity, then persist. Up to three fixed-point
passes required stable bytes and an equivalent full AST. Host definitions came
from a fixed template; schema/API declarations used the same formatter and
shared their complete declaration bytes.

Formatting applied to new or replaced settings, types, and resolvers from
`init`, to update and `.dist` candidates, and to the historical inference
candidate. Mock owned the formatter independently of the Codex adapter and
JSONL/context protocols. It did not redefine B/U/N classification: valid
user-edited files remained acceptable to `check` and `serve`, and no public
`fmt` command or style override was added.

Historical proposal v2 recorded formatter name, version, and style. Read-only
application verified those fields and idempotence, then installed the exact
approved bytes after ordinary type and relation checks; it never reformatted or
restamped at install time. Invalid generated defaults could be retained only as
unusable originals, never selected as fallback candidates. Existing process
isolation, absolute deadlines, cancellation, recovery, capacity, and
transaction rules remained in force.

The exception was limited to StyLua 2.5.2, full_moon 2.2.0, and
full_moon_derive 0.11.0 under their specifically approved licenses; it did not
grant broad MPL/GPL, publisher, or SSH approval. Ordinary ABI 6 and MSRV 1.85, Mock MSRV 1.95, and the then
current project, bundle, API, and reference formats did not change. FMT-01
through FMT-24 and Gates FMT-A through FMT-E passed locally in the 84-command
common suite. Sources, measurements, independent Reviews A/B, and unexecuted
environments are recorded in the
integrated validation record.

## 31. CLI TUI and embedded GraphiQL program

Status: Accepted / Implemented. The normative source is
[ADR-0035](adr/0035-cli-tui-and-embedded-graphiql.md). CLI grammar and help were
unified in clap definitions while command execution, redacted diagnostics, and
GeneratorInfo stayed separate. No parallel handwritten parser or help table
remained in either then-current CLI or example benchmark arguments.
Manifest-only authority, exclusive configuration, non-TTY behavior, hidden
worker protocols, and side-effect-free help/version output were preserved.

The historical `infer`/`apply` flows and reference viewer used a shared ratatui
layer. JSON, pipes, files, `--no-pager`, and non-interactive reference output
bypassed it. The UI did not own validation, approval, or placement. It retained
complete-text review, phase-separated typeahead, cancellation and job recovery,
terminal restoration, exact candidate bytes, and B/U/N, `.dist`, and recovery
protection. ADR-0044 later removed the built-in inference UI.

The common endpoint builder gained a default-disabled GraphiQL option.
Enabling GraphiQL explicitly exposes both the UI and introspection; final
introspection visibility is the explicit-introspection setting OR the GraphiQL
setting.
generated resolvers and `IntoEndpointInput` never copied HTTP or GraphiQL
configuration into a generated crate. A neutral HTML/fixed-asset component and
Axum/Hyper route helpers attached GraphQL, GraphiQL, and asset paths to the same
endpoint. The application continued to own listeners, authentication, CORS,
runtime, and shutdown. GraphiQL does not bypass the existing GraphQL
authentication, CSRF, rate, safelist, or budget behavior.

Pinned GraphiQL assets were isolated behind optional features and required no
Node or network at Rust build or runtime. Their dependency lock, canonical
generation, inventory, hashes, licenses, embedding, and HTTP boundaries were
verified. `Endpoint::graphiql()` exposes the neutral view,
`graphiql::Paths` validates paths, and Axum/Hyper helpers expose routes or a
service. Defaults are `/graphql`, `/graphiql`, and `/graphiql/assets/...`; a
custom prefix must agree with the submission target. Mock enables the UI only with `--enable-graphiql`; the setting is not
persisted in projects or bundles. A numeric loopback listener allows only its
exact own origin and never guesses Host, accepts wildcards, or trusts another
origin. Readiness completes before UI service starts while preserving the same
worker, state, rollback, and drain contracts; opening the UI does not initialize AI.

All DX35 IDs and Gates DX35-A through DX35-E passed locally in the 84-command
suite. Eighteen ordinary packages passed on Rust 1.85 with exactly the three
license exceptions above. The 2026-09-09 designer review, scoped to ADR-0035
and Section 30, reported zero findings at every severity. Asset and UI
manual verification are recorded separately from unexecuted remote CI in the
integrated validation record.
Completion did not require changes to external applications, publisher or SSH access, or
real AI input.

## 32. Mock Luau hooks and JSON logging program

Status: Accepted / Implemented. The normative source is
[ADR-0036](adr/0036-mock-luau-hooks-and-json-logging.md). It integrated six
hook slots, declared state, B/U/N handling, bundle v4, isolated VM workers, HTTP
plugins, numeric status policy, bounded hook/access JSONL, and API/reference v3.
The common plugin order is before hook, Rust `next`, then after hook; Luau never
receives a continuation. Shared contexts borrow bounded body and parent views,
and `FieldError` exposes only safe message and code summaries. Extensions,
internal sources, alternate executors, and Mock dependencies do not enter core.

`request.before` and `operation.before` may read the request, models, and state
and return a typed replacement state patch. `field.before` may read them but
cannot update state. Every before slot can reject stickily. A rejection status
is a numeric HTTP status; 200 represents a GraphQL denial and invalid statuses
normalize to 500. Concurrent rejections choose the numerically greatest
status. Request-local Mock metadata determines the final status; after hooks
and extensions cannot override it, and an unrelated transport failure cannot
be downgraded. A status that forbids a body produces an empty body. After hooks
observe models, state, and phase results read-only and cannot reject, replace a
result, or modify store or state. `field.after` observes its result before child
selection completion. Request state begins
from configured defaults, is isolated per request, and changes only through
the approved before phases. The store snapshot and Mutation-root commit point
are fixed, and after hooks run best-effort within the remaining absolute
budget. Field hooks exclude internal typename and
introspection fields; request and operation hooks still apply. VM, worker, and
queue work share one absolute deadline and bounded cancellation/recovery
contract.

Hooks live in a fixed directory and registry connected to `init`, `update`,
`check`, and `export`. Disabled hooks remain in the ownership, capacity,
isolation, and transaction set. Update retains B/U/N and `.dist` protection,
and formats only new Luau through ADR-0034. A successful update does not imply
readiness. The same host definitions generate phase-specific types, checks,
and reference contracts. Common/schema reference input reports generic state;
project/bundle input reports actual closed declarations and active
registrations without exposing initial state values or hook source. Reference
generation does not check readiness, and document construction and rendering
remain separately bounded at 16 MiB.

The historical inference path selected active hooks explicitly and could not
change the registry, configuration, or state authority. Proposal v3 bound
configuration, state declarations, defaults, and registration in a read-only
stamp and retained exact approved bytes and U-state protection. Provider events
were normalized into bounded observation events; raw stderr, credentials, and
hidden reasoning were neither displayed nor persisted. The ratatui and non-TTY
views kept bounded histories and distinguished unverified text from adoptable
candidates. ADR-0044 later removed this inference path.

`serve` writes hook and access JSONL to stdout and startup or human diagnostics
to stderr. It never adds raw bodies, documents, headers, tokens, state, or
results automatically. Hook and access queues each hold 64 records plus eight
notices, with 16 KiB per record. Separate stdout and stderr child processes are
used; the parent holds at most one unacknowledged frame per writer and
distinguishes write acknowledgement from flush acknowledgement. Shutdown uses
the shared five-second bound while distinguishing child, process-group, and
join completion. The HTTP-body wrapper records the actual response status and
whether body delivery completed or was dropped. Timestamps are UTC Unix
milliseconds. Authorization preflight uses explicitly allowed origins and does
not infer wildcards or credentials.

This program advanced project v2, bundle v4, API/reference/proposal v3, and
update-state/schema-snapshot v2 while preserving ABI 6, SchemaDigest semantics,
ordinary MSRV 1.85, Mock MSRV 1.95, old-reader rejection, and nondestructive
user files. All HK IDs and gates, the 85-command suite, Reviews A/B, debug and
release recovery, two-domain/rename cases, and the 115-request macOS release
run passed locally. The
integrated validation record distinguishes
that evidence from real AI, SSH, new-browser, and remote-CI work not performed.

## 33. Mock TUI usability and Unicode program

Status: Accepted / Implemented. The normative source is
[ADR-0037](adr/0037-mock-tui-usability-and-unicode.md). This ratatui program is
an independent usability improvement and does not reopen earlier Passed gates.
When implemented, it replaces Section 29's earlier non-ASCII display behavior.
The shared infer/apply/reference `cli/ui` separates screen state, Unicode
editing, diffing, and source-view coverage. Logic adapters and project
transactions do not know terminal layout or keys and preserve transmitted bytes
and source positions.

Every screen has a fixed status/footer and supports lines, checkboxes, cursors,
selection counts, and multiline fields containing arbitrary Unicode. Shared
navigation uses `j`/`k`, arrows, page keys, and `g`/`G`. Editing and deletion
distinguish UTF-8 source bytes, grapheme clusters, and display cells. The UI
does not implement custom IME behavior or Unicode normalization. Dangerous
characters are visualized so they cannot imitate source boundaries, literal
escapes, or terminal controls. Diffs use bounded line-oriented unified output
with optional color, prefixes, and line numbers; `NO_COLOR` remains supported.
The UI shows full text and line-ending differences, falls back to full content only with a
notice at the search limit, and never presents an end jump, resize, or clipped
view as complete coverage.

The workflow distinguishes unselected state, pre-transmission verification,
inspection or post-review adoption, final explicit approval, cancellation,
bounded I/O, and recovery. If Section 32's communication display exists, it
uses the bounded body while progress and cancellation stay in fixed screen
regions. ADR-0036 owns communication and hook access; their presence is not a
precondition for ADR-0037. Non-TTY behavior, CLI and storage formats, and ABI 6
remain unchanged. Product MSRV 1.85, Mock MSRV 1.95, the 400-line handwritten
limit, and existing display/input/resource bounds remain in force. All UX37 IDs
and gates passed locally; handoff and conformance are recorded in Section 25.
Real AI, a new browser base, SSH, and publisher gates are not added. Automated
verification, manual UI verification, and designer approval remain distinct.

The implementation connected the shared screen, list, status/footer, Unicode
editor, and complete pre-transmission text. Large pastes are inserted as one
unit; ordinary character sequences are grouped into bounded read units, while
ratatui renders cell-level differences. Real-PTY tests covered 15,000
characters, original UTF-8 and transmitted-byte hashes, cancellation, and
reset. The bounded-LCS unified diff, byte views for both sources, line-ending annotations,
and colorless display were also exercised through real PTY adoption/apply.
Local acceptance passed. The 2026-09-10 ADR-0036/0037 designer review recorded
zero findings at every severity. Human UI/IME and remote CI remain unconfirmed;
ADR-0037 Sections 6.1 and 6.2 and the
integrated verification record retain the
exact limits and results.

## 34. Simplified Mock directives and model API (Accepted / Implemented)

[ADR-0038](adr/0038-simplified-mock-directives-and-model-api.md) is the
approved follow-up design. Earlier descriptions in Sections 26 and 27 are not
evidence of compliance with it. ADR Section 10 and the
SD-0 decision record fix the detailed
contracts. The model/API/format transition and integrated local acceptance with
Section 35 are complete. The proposed 3/4/1/4 observation-budget allocation was
not adopted.

Concrete objects become models with empty seed files and may use arbitrary IDs.
Seven operations share one `QueryPlan`. Explicit
relation declarations include both owner and inverse fields, and the IR matches
only consistent declarations. Unmarked objects and lists remain embedded
values; implicit many-to-many storage is internal. Automatic IDs are generated
only for eligible runtime writes, never for seeds or explicit IDs. Fifteen
constraint arguments and eight formats share closed semantics across fields, references,
virtual values, generated Luau, and runtime validation. The implementation does
not claim Prisma, JavaScript, or every GraphQL ecosystem behavior. Unsupported
regex syntax is rejected before startup under local and shared resource limits;
legacy CRUD mappings, aliases, automatic format conversion, `errorMessage`, and
`uniqueTypeName` are absent.

The typed API registry is the single authority for model, slot, key, selection,
and constraint meaning. Luau declarations, static and runtime checking,
reference output, and the historical inference context all consume it. Updates
retain explicit generations, edited U files, `.dist`, recovery, read-only hook
views, transactional roots, and VM cancellation/recovery. Ordinary ABI 6 and
MSRV 1.85 stay separate from Mock MSRV 1.95. The
regex evaluation is selection evidence, not
product verification.

Root DTOs without IDs or reference keys are validated under the same rules as
stored values and retain read-only snapshot semantics. Relation edge order is
explicit: no-ops preserve order and a removed
then reconnected edge moves to the end. Candidate transactions include order
and index capacity, so a failed root discards records, indices, counters, and
order together. Constraint and regex rules flow from executable authority into
text and JSON reference arguments. A sticky observation-budget failure emits
one bounded diagnostic and stops the remaining after hooks. For an ordinary
exception while the budget is healthy, outer after hooks continue, and the
stopped state is not exposed to before hooks or business results. Section 35 supersedes ADR-0038's earlier
root-by-root exchange and observation-frame failure behavior. Final evidence is
in the ADR-0038 validation record.

## 35. Request-atomic Mock execution (Accepted / Implemented)

[ADR-0039](adr/0039-request-atomic-mock-execution-and-soft-hard-limits.md)
governs request-atomic execution. Every Mutation root in one request operates on
one candidate state under one request-scoped Mutation permit. The state is
published only after the complete response and a final cancellation check
succeed. Other requests never observe the candidate;
each Query uses one committed snapshot. Hook after-phases and stored views are
read-only. Any later exception, invalid return, hard failure, or cancellation
rolls back the complete candidate. A GraphQL response that would otherwise be
partially successful becomes a total Mock failure without changing ordinary
endpoint or ABI 6 behavior.

The rejected 3:1 reservation scheme was removed. All work returns to one
cumulative ledger: crossing 75 percent emits a bounded warning and continues,
while the existing hard limit or deadline rejects and rolls back with status
500. Explicit `reject` with status 200 is a deliberate GraphQL denial with
`data: null`, errors, and rollback. Type, input, authorization, relation,
reference, VM, and return failures remain hard, and a selected status cannot
hide an internal failure. Soft warnings never authorize unbounded continuation.
Post-commit send disconnection, stderr rollback, and a synthetic normal
response after forced process termination are outside the guarantee.

The seven format families, reference output, and usage procedures were updated
with the current layout and key formats. Old readers, aliases, fallbacks, and
automatic conversion are absent. Current model operations, GraphQL semantics,
edit protection, and U/`.dist`/backup/index/validation behavior remain intact.
The goal prompt and
validation record map RA-01 through RA-18.
All IDs and gates, the 102-command suite, measurements, independent Reviews A/B,
and revision re-review passed locally.
The designer's final implementation review Passed on 2026-09-14 with zero
findings at every severity.
Remote CI, human UI/IME testing, and real AI connectivity remain unverified.

## 36. Mock scalar representation directive (Accepted / Implemented)

[ADR-0040](adr/0040-mock-scalar-representation-directive.md) renames
`@chigiri_mock_scalar(representation: ...)` to
`@representation(type: ...)`. It preserves seven representation values, six
constraints, constraint composition, and request-atomic behavior. Vocabulary
v3 and both v5 management formats reject the old name, old argument, old
readers, aliases, and automatic conversion. ABI 6, ordinary MSRV 1.85, Mock
MSRV 1.95, and the host API retain the ADR's version boundaries.

REP-01 through REP-12 and Gates REP-A through REP-C passed locally. Item-level
reference navigation was intentionally left to the separate
research and proposal. Commands, source
hashes, same-release measurements, independent review, and unexecuted
environments are in the
validation record. Local acceptance
and designer review passed on 2026-09-15 with no unresolved finding.

## 37. Mock reference topics and navigation (Accepted / Implemented)

[ADR-0041](adr/0041-mock-reference-topics-and-navigation.md) defines structured
reference presentation. Default text is a short landing page for six topics;
`reference all` returns the complete text; `[TOPIC] [NAME]` selects closed lists
and details for directives, methods, models, hooks, types, and limits.
Selections preserve explicit input, exclusion, and unverified-readiness
boundaries.

The composed semantic schema supplies five GraphQL standard directives, four
Mock directives, and user declarations. Syntax, locations, arguments, and
defaults derive from semantic definitions; closed choices such as `format`
come from the execution enum. Presentation metadata is authored once, and
user-defined directives never receive Mock execution semantics. Complete JSON advances to reference v6. `all` returns the full
document, while a topic selection returns a closed topic document whose content
matches its selection and resolves all local rule references. Proposal and
context identity always use the complete v6 document. Host API v5, project v4,
bundle v6, and ordinary generated ABI 6 remain unchanged.

The built-in viewer navigates topic list, item list, and detail levels with
Enter and Backspace. `all` opens complete text in the same viewer. It retains
scroll/search keys, the `CHIGIRI-MOCK-REFERENCE(1)` title, man-style sections,
`(END)`/`--More--`, terminal restoration, bounded I/O, and `NO_COLOR`.
Non-TTY, JSON, and `--no-pager` output bypass the viewer. REF-01 through REF-15
and Gates REF-A through REF-D passed locally on 2026-09-15. The designer's
Medium 3 and Low 1 findings were partly cleared on rereview; the final U+061C
bidi-visibility fix, including regressions for all 12 bidi controls, still
awaited designer confirmation. The validation record
preserves this evidence without reopening ADR-0040. Remote CI and human
TUI/IME verification remain unexecuted.

## 38. Mock type-check batching and initialization performance

[ADR-0042](adr/0042-mock-typecheck-batch-and-init-performance.md) batches
independent Mock module checks without modifying third-party sources. Each source and expected-type wrapper is
isolated in a function and `do` scope, while the common host definitions are
passed once to the public one-shot type-check API. Standalone compilation still
runs first, so batching cannot make wrapper-escape syntax valid. The batch maps
bounded diagnostics back to the original resolver or hook and preserves
capacity, cancellation, the process gate, readiness, and per-module execution
VM initialization. Measurements and unexecuted environments are recorded in
the performance validation record.

## 39. Composable agent-ready Mock authoring

[ADR-0044](adr/0044-composable-agent-ready-mock-authoring.md) makes the editable
project directory the integration boundary for coding agents. `init` emits a
bounded `AGENTS.md` that identifies editable and read-only paths. The document
does not participate in runtime configuration, bundles, or readiness.

`reference types TYPE`, `reference models MODEL`, and
`reference resolvers PARENT.FIELD` expose schema-derived contracts in bounded
text or closed JSON. Reference output goes directly to stdout. Chigiri does not
own a terminal pager or interaction session.

The ADR-0044 baseline emitted `chigiri-mock-check/v1`. Repeated exact
`check --target PATH` values validate active project files against the same
Luau, hook, and connected-seed contracts used by full readiness. Target success
may report `readiness_checked=false`; only a full successful check authorizes
`serve`.

Agent reasoning, model selection, timeouts, editing, and diff review belong to
the external agent and its host. The built-in inference backend, proposal and
adoption protocol, interactive review UI, and reference viewer are absent from
the current binary.

## 40. Stateful Mock authoring and runtime parity

[ADR-0045](adr/0045-stateful-mock-authoring-and-runtime-contract-parity.md)
is Accepted / Implemented. One versioned capability registry supplies the Luau
checker declarations, runtime sandbox registration, and runtime reference
topic. Bounded wrappers cover native string, pattern, allocation, and table
operations without exposing filesystem, process, network, environment, debug,
or dynamic-loading APIs. The checker binds registry types directly except for
`assert`, `error`, and `type`, whose pinned checker intrinsics preserve flow
refinement that an ordinary function alias cannot express; their published
names and signatures still come from the same registry and are covered by
checker/runtime conformance tests. Runtime wrappers normalize the VM's wider
native behavior: `assert` enforces the intrinsic's optional string message and
single-value return, and `string.find` returns only the published index pair.

The evaluated project has distinct public `schema.sources` and private
`storage.sources`. Mock IR and CRUD use the combined graph, while public
validation, dispatch, introspection, and response types use only the public
graph. Generated metadata binds both schema roles, fixed/system clock settings,
state, generated types, and `chigiri-mock-capabilities/v1` into its identity.
Public and storage sources share the existing aggregate 64-source and 128-KiB
SDL boundary; the two configured lists do not each receive a separate budget.
Storage SDL is AST-closed to private object, enum, and represented-scalar
definitions plus field-only object extensions. Operation roots and every other
definition or extension form are rejected before public and storage graphs are
combined.

Every resolver and hook receives one readonly request snapshot and one captured
application clock. Model-backed tables retain host provenance when placed in
ordinary output containers, so nested relations remain live without accepting
forged, foreign, stale, or modified tables. The codec pins the exact originating
VM table for the request lifetime, preventing garbage-collection address reuse
from transferring provenance. Application time does not affect monotonic
execution deadlines. Seed `$ref` markers resolve deterministically before store
construction and portable export. A sticky `ctx.fail` business error is the
only resolver-controlled public code/message path; request rollback and
independent internal/resource-failure precedence remain unchanged.

The clean-break surfaces are project v5, bundle v7, host API v6, full reference
v7, topic reference v3, update-state/schema-snapshot v6, check v2, and runtime
log v2. Earlier inputs are rejected before mutation. Migration is creation of a
fresh directory plus explicit copying/adaptation of user-owned sources; current
`.dist`, transaction, and recovery protections remain authoritative.

## 41. Standard time and explicit timeleap control

[ADR-0046](adr/0046-standard-os-time-and-explicit-timeleap-control.md)
supersedes only ADR-0045's application-clock surface. The sandbox exposes the
pinned native `os.time`, `os.date`, `os.difftime`, and `os.clock` behavior
through bounded wrappers. Time substitution applies only to `os.time()` and to
`os.date` when its timestamp is omitted or nil. Explicit conversions,
`os.difftime`, `os.clock`, host deadlines, logs, generated identifiers, and
shutdown timing retain their independent clocks. Initialization permits pure
explicit conversions but rejects live time reads and control.

Each server owns an immutable startup policy and a mutable current policy.
Each request captures the current server policy before `request.before` and
shares an optional local override across all of its VMs. Local control is
available only during request preparation and is sealed before field
execution. Local reset resumes that request's captured policy. The dedicated
host controller serializes candidate executions and publishes once after the
final failure and cancellation fence; `world = true` affects only later request
contexts in that server. World reset restores the immutable startup policy.
No GraphQL input, hook, header, CLI route, or default option grants world
authority, and server instances in one process remain independent.

The capability registry is the common source for sandbox installation,
phase-aware checker declarations, generated types, and runtime reference
signatures. Discovery names `runtime stdlib` for `os` and `runtime timeleap`
for control; the removed `runtime clock` and `ctx.clock` are not aliases.
Startup configuration remains declarative as System or a fixed integer Unix
timestamp in seconds. Export records that startup declaration, never live
server or request state.

The clean-break surfaces are project v6, bundle v8, host API v7, capabilities
v2, full reference v8, topic reference v4, and update-state/schema-snapshot v7.
Check v2, runtime log v2, generated Rust ABI 6, and the existing MSRV boundary
are unchanged. Earlier and mixed artifacts are rejected before mutation;
migration uses a fresh directory and explicit adaptation of user-owned sources.

## 42. Mock authoring type and output boundaries

[ADR-0047](adr/0047-mock-authoring-type-and-output-boundaries.md) is Accepted /
Implemented. Its final-source evidence is recorded in
Mock authoring boundaries validation.

Generated record shapes must compose each storage slot once. Checker-only
contextual typing must accept ordinary inline predicates and heterogeneous
order arrays, including indexed calls, without weakening invalid-input checks
or changing runtime evaluation and source mapping.

Output handling must distinguish unchanged authoritative records, trusted
embedded values read from stored slots, and arbitrary output DTOs. Stored
private fields remain internal and available to the appropriate nested parent
without being serialized publicly. Embedded values remain snapshots and do
not gain model identity or live relations; authoritative record provenance
remains strict. A private extension alone must not force an embedded field
to become virtual.

Public SDL remains authoritative for stored, relation, and virtual behavior.
Derived viewer values and live lookups require appropriate explicit SDL
annotations; storage SDL and mock configuration cannot override a public
field's behavior. Generated and reference guidance explain this boundary.

The implementation adds exact named hook discovery and safe, specific local
diagnostics using verified script locations rather than fabricated paths. At
the ADR-0047 checkpoint, the contracts were runtime log v3, reference v9/topic
v5, check v2, project v6, bundle v8, host API v7, capabilities v2, and update
state/schema snapshot v7. Section 46 defines the superseding current Mock
identities. Generated Rust ABI 6, clocks, resource limits, and request
atomicity retained their existing contracts at this checkpoint.

## 43. Chigiri brand and single-binary CLI

[ADR-0048](adr/0048-chigiri-brand-and-single-binary-cli.md) is Accepted / Implemented.
It defines a product-wide clean-break rename to Chigiri with one public product
binary, `chigiri`, composed by a new `chigiri_cli` adapter package. The adapter
calls the codegen and Mock libraries directly; it does not launch the former
Cargo-plugin or standalone Mock executables.

The public command surface becomes:

```text
chigiri rust init | codegen | update | check | doctor | scaffold
chigiri mock init | update | check | serve | export | reference
```

Each alternative is a separate subcommand. Rust `generate` becomes `codegen`;
`scaffold projection` becomes `scaffold --object Book`, retaining stdout-only,
single-object projection behavior. Exact-output `check` and consumer-aware
`doctor` retain their distinct responsibilities. Bare root/namespace commands
show help. There are no old binary, Cargo-plugin, or command aliases.

Remove Rust schema-governance `snapshot`/`diff`, their exclusive public APIs,
formats, classification code, and dedicated gates. Preserve Mock's internal
`generated/schema.snapshot.json` and update comparison, Rust schema digests,
binding/resolver fingerprints, ABI 6, generated ownership/exactness, independent
catalog validation, and testkit/runtime checks. Shared tests must retain their
unrelated assertions through retained APIs rather than disappearing with a
governance helper.

The CLI and Mock use Rust 1.95 outside the default MSRV-1.85 member set; normal
libraries, the codegen library, and generated consumers retain MSRV 1.85 and
their existing dependency boundaries. Rust workflows still require their
existing Cargo/rustup/fixed-rustfmt tooling. Mock keeps its embedded runtime and
assets, and its hidden writer launches the same current executable. One binary
does not imply one process or remove the external tooling needed to build Rust
applications. Linux/macOS CLI host scope is unchanged.

Rename active product-owned packages, generated names, configuration/marker
files, format prefixes, and private operational identities together. Retain
numeric format versions where shapes/semantics are unchanged, while rejecting
old or mixed branded inputs before mutation. Recompute renamed fingerprint
inputs and regenerate canonical outputs through their producers. Migration
uses fresh destinations and explicit adaptation of user-owned sources, not an
automatic converter or overwrite of existing projects.

ADR-0048 supplies the exact supersession, exclusions, and CH-01 through CH-11
acceptance requirements. Historical records retain their original identities.
Release packaging, registry publication, installers, and new production or
platform-support claims are outside this transition's acceptance scope.

CH-WP1 through CH-WP4 are implemented. Dedicated Rust governance is removed, and
the sole product target is `chigiri`, whose one closed Clap tree dispatches
directly into the codegen and Mock libraries. CLI-owned integration tests and
the Mock library/VM tests run in the explicit Rust 1.95 lane; normal libraries,
codegen, examples, and generated consumers remain in the Rust 1.85 lane. The
product/package/format rename is implemented in CH-WP3. Relocated Rust and
Mock workflows, the complete final-source validator, and both independent
reviews passed in CH-WP4; the validation record preserves exact scope and
unexecuted environments.

## 44. Actionable Mock authoring feedback

[ADR-0049](adr/0049-mock-authoring-modules-and-feedback.md) is Accepted / Implemented.
Section 46 describes the implemented successor for completion state and
inventory. The contract below records the historical pre-cutover baseline;
its refinement, diagnostics, and navigation remain supported.

Its revised scope retained terminal refinement, safe diagnostics, scoped
reference navigation, and unfinished-entry inventory. Project-local `require`
was considered and withdrawn: the adopted published luaur API lacks the safe
cross-file checking surface, and added upstream maintenance or a Chigiri-owned
module adaptation system is not justified for this optional convenience.
This is not a limitation attributed to the Luau language itself.

No loader, import graph, module cache, library target, module export, or
commented library sample is introduced. The old public-release module gate is
historical, not a dependency for finishing the reduced scope. Retain unmodified
registry luaur dependencies and existing unsupported-loader behavior; no
vendor/patch/fork/Git, copied internals, or unsafe-wrapper workaround is allowed.

Checker-only terminal handling corrects genuine `ctx.fail` refinement without
changing runtime evaluation or suppressing real nullable failures. Typed
same-file helpers remain covered. Classification uses published diagnostic
kinds where available or independently proven AST/contract facts. Unclassified
native errors use safe general wording rather than message parsing or guessed
nil explanations. Original source spans and existing runtime/HTTP secrecy remain
authoritative.

Check and inventory diagnostics gained at most four distinct, relevant registry
reference selections with matching text guidance. Reuse existing reference
topics/formats, omit guesses, and do not evaluate configuration again for
navigation. Existing full and target checks retain their readiness semantics.

The former `check --list-unimplemented` enumerated active resolver and enabled-hook
descriptors without seeds or invocation of their bodies. It returns known plus
unclassified work, enforces aggregate limits and cancellation, and distinguishes
complete enumeration from readiness. BrokenPipe handling remains successful.

At that source identity, check v3 and unfinished-inventory v1 were introduced. Project v6, bundle v8,
host API v7, capabilities v2, reference v9/topic v5, runtime log v3, update
state/schema snapshot v7, and generated ABI 6 remain unchanged. No project
migration is required; ownership, update, recovery, and input identities remain
protected. Module-only planned version increments are withdrawn.

The public check-v3 and inventory-v1 surfaces, generated guidance, retained
workflows, comparable measurements, two independent reviews, and the complete
local validator were implemented. AF-08 through AF-11 and AF-14 through AF-16
pass locally on macOS; the other AF IDs are retired by scope amendment. No
module release wait, scenario runner, daemon, or persistent cache is part of
the delivered contract.

## 45. Portable host boundary and tiered CLI builds

[ADR-0050](adr/0050-portable-host-boundary-and-tiered-builds.md) is Accepted /
Implemented. A publishable Rust-1.85 `chigiri_host` support crate owns portable
logical paths, capability-relative filesystem operations, identities, locking,
bounded child processes, interruption, and isolated-output primitives. Codegen
and Mock retain their domain transaction and workflow policies but no longer
own platform branches or direct OS APIs above this boundary.

The root Make interface has native `release` and `debug` targets that leave
their binaries under `target/release/` and `target/debug/`, never under `dist`.
Their preflight rejects a resolved Cargo `build.target`; explicit target output
and empty encoded compiler flags prevent lower-priority Cargo configuration
from changing the native layout or compiler inputs.
Only named distribution targets populate the closed
`target/dist/<target>/chigiri[.exe]` inventory for macOS arm64/x86_64, Linux
x86_64 with glibc 2.17, and Windows x86_64 MSVC. Linux distribution builds use
the pinned compatibility environment even on native Linux. The complete
`make cross` runs on prepared macOS hosts; named targets have explicit build
host, SDK, linker, and tool prerequisites with no implicit acquisition.

Local `verify-dist` checks all artifact structures and applicable native Tier 1
smoke cases, recording foreign Tier 1 and all Tier 2 runtime checks as
unexecuted with reasons. It neither rebuilds nor substitutes for native Tier 1
qualification. `chigiri_host` must be packaged for future publication, but
actual registry uploads remain a separate release task.

macOS arm64 and Linux x86_64 are Tier 1 and require the complete validator on
native final-source environments, with two required CI lanes. macOS x86_64 and
Windows x86_64 MSVC are build-only Tier 2 targets: release binaries must build,
link, and have the expected Mach-O/PE identities. Test-executable linking and
native runtime checks are not required. Fail-closed design constraints remain
unchanged, without treating compilation as runtime safety evidence. HOST-01
through HOST-14 are complete within the evidence boundaries recorded in the
ADR-0050 validation record.

## 46. Mock contract validation without completion state

[ADR-0051](adr/0051-mock-contract-validation-without-completion-state.md) is
Accepted / Implemented. Resolver modules return a
closed table containing only `resolve`, and hook modules return a closed table
containing only `run`. Their completion `state` and corresponding bundle,
candidate, generated-type, reference, and progress-report fields are removed.
An old descriptor is rejected, not accepted with ignored state.

Generated resolver bodies use existing `ctx.fail` with code `NOT_IMPLEMENTED`
and fixed public message `This operation is not implemented.` Generated hooks
remain no-ops returning `nil`, with all six registrations initially disabled.
These are ordinary implementations, not markers inspected by the product.
No unfinished inventory, replacement scanner, or implicit completion registry
remains. Structural active-file inventories needed for ownership are preserved.

Full validation still checks configuration, schema, seeds, source types,
closed descriptors, bounded initialization, verified identities, and resource
controls without invoking entrypoint bodies. For otherwise valid inputs,
intentional-error resolvers may pass check, serve, and export. Target success
and init/update placement do not certify full readiness. `serve` and `export`
perform full validation of current inputs themselves.

Invoking an intentional-error stub follows existing business-failure handling,
including HTTP 200 with a GraphQL error and request-wide rollback. Ordinary
internal errors remain sanitized HTTP 500 failures. Request-local `ctx.state`,
`statePatch`, hook registration/permissions, stored data, clocks, and update
ownership/recovery are unrelated to completion state and remain unchanged.

The clean break introduces project v7, bundle v9, host API v8, reference
v10/topic v6, and update-state/schema-snapshot v8. Check v3, capabilities v2,
runtime log v3, vocabulary v3, and generated Rust ABI 6 remain unchanged.
Old/mixed inputs fail before writes; migration initializes a fresh directory
and deliberately adapts user-owned sources without copying old generated data.
No compatibility reader, in-place upgrade, or dependency change is introduced.

CV-01 through CV-09 are implemented and tracked in the
ADR-0051 validation record.
ADR-0049's inventory acceptance is historical, while its retained feedback
improvements remain required. The host/build transition in Section 45 remains
independent, and non-local platform validation is outside this local cutover.

## 47. Adopted schema snapshots and explicit Mock code generation

[ADR-0052](adr/0052-adopted-schema-snapshots-and-mock-codegen.md) is Accepted /
Implemented. It supersedes Section 28's external-source runtime
authority and management-independent project reference, and Section 40's
storage-update workflow. ADR-0051 remains the current implementation baseline.

The existing schema snapshot gains normalized, description-preserving public
SDL and its own freshness digest. Its summary, combined schema identity,
configuration stamp, declarations, and ownership metadata remain derived and
verified together. A shared bounded normalizer preserves decoded strings and
source/token order while dropping ignored formatting and comments. Public
freshness and internal generation integrity are separate identities.

`init` adopts initial public SDL; `update` adopts current public SDL and
regenerates once; `codegen` regenerates from adopted public SDL and local
storage/settings without opening public source paths. Both writers share
existing edit protection and transactions, including support for unfinished
authored resolver/seed migration. `update --check` is a read-only public
comparison, not readiness or a generation dry run.

Storage paths are confined to the retained project directory. Project reference,
full/target checks, serve, and export require internal generation consistency,
not external public-source availability or freshness. Project reference still
does not inspect resolver bodies or seeds. Local contract drift directs authors
to codegen; invalid ownership fails closed without fallback. Reference projects
descriptions from the same graph as their contracts, retaining private visibility
and safe bounded rendering. Bundles remain self-contained.

ADR Section 8 defines the clean-break project/metadata/reference versions;
bundle v9, host API v8, check v3, and ordinary Rust ABI 6 remain unchanged.
Migration uses a new directory, not an implicit adoption or compatibility
reader. AS-01 through AS-10 passed locally, and both independent reviews closed
after focused correction re-review, as recorded in the
ADR-0052 validation record.

## 48. Trait-only Rust integration and application boundaries

[ADR-0053](adr/0053-trait-only-resolvers-and-application-boundaries.md) is
Accepted / Implemented. Its coordinated generated ABI 7 implementation replaces
projection field implementation and scaffolding in Sections 5, 20, 21, 23, and
43. Historical validation remains evidence only for its recorded source.

Generate complete `ObjectResolver<C, T, E>` traits and private adapters.
Applications implement ordinary `async fn` methods returning
`Result<ExactOutput, E>`, retaining current argument, parent, lifetime, and
nullability rules. Every field is required. `SchemaTypes` continues to select
domain backing values; no output-object DTO, new getter trait, automatic
binding, derive, reflection, or domain-source inspection is introduced.

Remove the projection namespace, builders, setters, field-stage internals,
projection-specific `ResolverFuture`, and the public Rust scaffold command and
library API. There is one resolver implementation form with no compatibility
mode. Ordinary group assembly remains compile-time complete and schema-ordered.

`Resolvers::<C, T>::builder(mapper)` infers one error type `E` for the assembly.
Require `E: Error + Send + Sync + 'static` and
`mapper: Fn(E) -> FieldError + Send + Sync + 'static`. Each group must implement
its trait for that same `E`. Private adapters convert each resolver-returned
error exactly once before existing field-error handling; the completed public
resolver type remains `Resolvers<C, T>`. `FieldError` can be selected with an
explicit identity mapper. Do not expose arbitrary error text automatically.

Framework conversion/completion/middleware errors stay `FieldError` and bypass
the mapper. Preserve masking, original-source observation, error IDs, paths,
null bubbling, and list-item errors. The mapper is not a new executor error
policy or HTTP preparation hook.

Context setup uses existing `context_factory` functions/closures and
`PreparedContext::execution_only`, with compiled named-function and shared-`Arc`
closure recipes. Keep contexts request-local and retain `PrepareError`, response
hooks, timing, and cancellation. No new context construction API is required.

Retain standalone generated-crate ownership, exact checking, semantic output,
resource/MSRV boundaries, and application-owned server lifecycle. Retire
projection/scaffold guidance and regenerate maintained examples at cutover.
Mock format contracts remain unchanged. Focused RI-01 through RI-08 evidence is
in the validation record. Required focused
quality gates and both independent reviews passed locally. IDE and non-local
platform checks remain unexecuted.

## 49. Subscriptions and bounded stream delivery

[ADR-0054](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md) is
Accepted / Implemented with local macOS acceptance. The current source extends Sections
2, 8, 9, 11, and 12 with generated ABI 8 and the approved coordinated Mock
identity cutover. Former parse/reject tests are historical. Handoff Section 41
defines the implementation gates; Conformance Section 41 and the
validation record distinguish
executed source-bound checks, final identities and excluded qualification.

The implementation supports typed source initialization followed by ordinary selected
field execution for each event. Keep domain associated types, complete traits,
one application-error mapper, and core standard-library-only execution. The
common endpoint supplies policy and lifecycle; Hyper/Axum adapt WebSocket, SSE,
and multipart protocols. Rust applications retain listener, routing, runtime,
HTTP-version, connection, TLS, and shutdown ownership.

Prepare context once for setup and freshly for every event. Do not reuse setup
or preceding-event caches, principal, state, identities, or budgets. Retain only
bounded subscriber metadata, the source, operation, and subscription control
state. Source initialization runs once; event preparation may revalidate the
original credentials. Changed credentials require resubscription. Header hooks
run before streaming, not as per-event header mutations. Separate setup/event
deadlines from subscription queue/send lifecycle.

Add subscription settings to the existing typed component graph. Builder sugar
and `inject` have identical leaf operations, defaults, call-order replacement,
redacted final validation, and effective-configuration origins. Evaluate all
applicable limits with OR semantics. Queue capacity/age violations may be
recoverable by explicitly selected DropOldest; recheck every condition after
eviction. Oversized events and send stalls terminate; new-subscription quota
violations reject admission without evicting existing subscriptions. Default
backpressure never silently discards events and terminates when bounded demand
cannot be maintained. Normally only the affected subscription terminates, not
other operations on the same usable WebSocket.

ADR-0054 Section 5 fixes the initial safety defaults and configuration hard
maxima, including 128 active subscriptions, 32 queued events/256 KiB per
subscription, and a shared 64 MiB encoded-buffer budget. These are not measured
capacity. Invalid numeric settings or combinations return construction errors
before serving; no panic or silent clamping. Operational pressure on a valid
Endpoint follows the policy above, not application failure. Setup and each
event have their own deadlines, not a total subscription lifetime deadline.

Use one portable evaluator and delivery controller rather than per-protocol or
OS-specific rules. Timers must act without new events. Bound complete encoded
event queues plus separate in-flight/control staging; arbitrary domain heaps,
upstream queues, network buffers, and client acknowledgement remain outside
those byte guarantees. DropOldest is server-selected per subscription and cannot
be requested by GraphQL arguments or remove a hard cap.

Each emitted event result carries
`extensions.chigiri.delivery.droppedTotal`: null without DropOldest, otherwise
the current cumulative discarded-event count. Count only wholly unsent event
results discarded by Chigiri's selected queue policy. Never evict partial sends,
control, or terminal messages. Metadata does not certify end-to-end delivery;
advanced-client recipes read extensions without adding internal fields to SDL.

Mock adds typed explicit publication from Mutation resolvers, with bounded
pending events included in the existing candidate/rollback unit. Fan-out begins
after full commit and cannot await slow clients inside the commit fence.
Failures before commit publish nothing; subscriber failures after commit never
undo data. Every event has its own read-only store/clock view, subscriber
request metadata, initial state, hook lifecycle, and budgets. Preserve Record
provenance and private-output rules across host staging. Keep Luau invocations
short and bounded. Published roots stay immutable; declared relations from
sealed captured Records automatically use the event's current committed
snapshot. Relation keys come from captured authoritative fields, never a
current-row root rebind, digest comparison, or an arbitrary DTO's ID. All
related lookups retain the same event snapshot and existing read-only/privacy
rules. No suspended VM, external network, automatic notifications,
scheduled scripts, durable replay, or new completion flags are introduced.

The independently reviewed SU-WP0 contract freezes concrete interfaces, pinned
profiles, remaining finite bounds and the changed/preserved identity table.
Canonical producers apply generated ABI 8, Mock project v9, bundle v10, API v9,
capabilities v3, reference v12/topic v8, log v4 and management v10. Only explicit
`mock update` accepts a verified v8 project for the protected one-step migration;
other retired/mixed readers fail closed. See the workflow and migration guides.
Focused local macOS acceptance and both final whole-goal reviews are recorded.
Broad performance, remote and full-product campaigns were not executed or
required. Platform support tiers remain as defined by ADR-0050. New reviews and findings use Beads under the
[review workflow](review-workflow.md), not the historical Markdown ledger.

## 50. Discoverable subscription configuration and lifecycle ownership

[ADR-0055](adr/0055-subscription-configuration-and-lifecycle-ergonomics.md) is
Accepted / Implemented with local macOS acceptance. The
validation record identifies current
numeric, managed-lifecycle, sizing, consumer and focused quality evidence for
SE-01 through SE-08, plus final source artifacts and scoped Beads reviews.
The following describes the implemented contract. Non-local and broad capacity
qualification remain unexecuted.

Expose typed associated bound constants on `SubscriptionLimits` and
`SubscriptionProtocolSettings`, backed by the same definitions as validation.
Keep `Default`, all existing limits and fit relationships, final graph
validation, and sugar/inject replacement semantics. Individual hard maxima do
not promise simultaneous capacity; never enlarge another budget implicitly.

Extend configuration diagnostics only for allowlisted numeric subscription
settings with typed range, fit, or checked-overflow evidence. Preserve slot,
origin and violation identity, and show exact values/units in Debug and Display.
This explicitly amends ADR-0017's no-values rule only for these settings;
all other slots and arbitrary strings, credentials, requests and provider state
remain redacted.

The SDK, not generated code or adapter-local policy, owns automatic supervision,
managed Query/Mutation and subscription work, timers, cancellation, completion
and bounded recovery. Official service construction activates the shared owner
fallibly in the existing supported runtime; it must not return an unsupervised
service. Clones share ownership rather than spawning another supervisor. Keep
low-level reservation, registry, emission and quarantine APIs internal.
Use the existing optional Tokio integration, retaining a Tokio-free default
facade/core. Listeners, routing, signal sources and server-wide admission remain
application-owned. Adapters only convert requests/bodies/sockets and native
lifecycle notifications; they do not implement independent shutdown policies.

The managed Axum service fits ordinary `Router::route_service`; infrastructure
failure before response commit becomes a bounded masked finite error, not a
successful stream. This explicitly supersedes Section 49/ADR-0054's mandatory
fallible connection-service propagation. Preserve source/hook once points,
reservations and abnormal post-commit body termination. Both adapters and
affected internal users migrate to one SDK owner, without parallel manual and
automatic integration. Existing finite-handler behavior remains supported.

For managed finite HTTP, the concrete SDK task arms its unique prepared
completion before post-context deadline checks. Cancellation drops in-flight
execution before one Failed/TimedOut completion attempt in that task. Callback,
panic-payload and application destruction stay outside native response waiters
and process recovery; actual join retains the original physical request charge.
Disarm before calling a completion so failure cannot retry it. The compatibility
finite handler keeps its existing cancellation behavior.
Failed disposal preserves the existing poisoned concrete-worker record and
physical ownership even after native join, rather than reporting released
capacity from an incomplete destruction attempt.

Managed HTTP and WebSocket validation keep application-bearing results in a
bounded handoff owned by the concrete validation task until the SDK accepts
their transfer. Native join results contain only closed completion or failure
classifications. Endpoint stop can finish an unaccepted handoff without another
connection poll; the original task destroys its abandoned result. Accepted
results and diagnostic owners are destroyed or transferred by the SDK before
the original pending worker slot can be released after actual join. Failed
disposal preserves that original poisoned worker and physical charge. Task
failure and private observation survive HTTP preparation abandonment; one
shared observation attempt covers the SDK ownership transfer without invoking
application work on process recovery. This handoff adds no worker, admission
pool, generated contract or adapter-specific lifecycle policy.

Managed WebSocket source and finite operation tasks bind failure reporting and
disposal integrity to their active generation, rather than the earlier
validation task. Their native joins contain only closed completion or GraphQL
response values. Child future and session disposal retains the same activity
evidence through actual join; a failed destructor preserves the original
poisoned worker, operation and byte owners. Managed source polling defers
natural terminal destruction to the guarded session boundary without inventing
a host observation receipt. Cancelling pending source demand guards the inner
future that temporarily owns the source. Ordinary execution timeout remains an
operation response and does not by itself fail the endpoint.

Managed context type erasure guards the raw application factory future before
awaiting it. A successful prepared value survives a failure in that future's
destructor and remains in the same concrete SDK task. Finite HTTP and initial
HTTP source preparation arm their unique completion before post-phase checks;
failed disposal then stops preparation before new executor/source/hook work.
The task disposes abandoned context, response state and remaining completion
fields independently, reports one closed infrastructure failure and preserves
its original poisoned physical owner through actual join. WebSocket continues
to suppress HTTP completion callbacks. The internal scope has only the existing
finite-HTTP and subscription task roles; no worker or future registry is added.
Neutral and legacy factory erasure keep their existing behavior.

WebSocket connection initialization retains authorization errors in its original
concrete SDK task. Native join carries only an optional authorization decision
and a closed worker failure. The task guards the raw future, observes and
destroys opaque errors there, and arms separate authorizer/diagnostic disposal
guards before the first poll. A shared private observation attempt is claimed
before invoking application code. Ordinary rejection or application error
closes only that connection; unexpected task/callback failure closes endpoint
admission. Failed disposal retains the original poisoned connection owner
through actual join. Scoped expiry does not add another cleanup wait or become
Drained after a blocked error destructor eventually completes. Protocol ACK
ordering and completion suppression remain in force.

Managed native WebSocket conversion uses the original connection generation
before authorization starts. A closed SDK task guards the raw upgrade future
and its destruction, retaining a successful socket until bounded acceptance.
Its native join carries only a closed unit result. Only actual join permits
the same connection worker slot to be reused for authorization. Attachment
uses the runtime captured at service activation; a pending entry retains only
endpoint state, not the managed service owner. Cancellation transfers the
original connection, retained opening metadata and original upgrade deadline
beside that concrete join into existing quarantine without starting another
cleanup timer. Failed disposal retains that original poisoned record. An
ordinary native conversion disconnect closes only the connection. This does
not include a native invariant failure, which fails endpoint admission. This
does not by itself establish the later opening, ACK or session disposal boundary.

SDK-managed socket entries also retain the same connection's cleanup ticket
and physical lease through socket and entry disposal. Connection cleanup
defers its completed original owner to that bounded scope; the closing path
destroys the native socket before actively releasing connection cleanup.
Authorization shares only the scope's resource-free failure evidence, so a
pending authorization still retires its original worker and ticket. Entry
polling, native invariant failures, socket destruction, private observation and
panic-payload disposal are guarded within the owning SDK task. Observers and
application state finish destruction before the scope releases ownership.
The terminal owner uses move-only optional inner resources and Drop-based
deferral, so interrupted cleanup-coordinator construction cannot release its
original ticket ahead of the same scope.

The existing fixed connection slots retain failed scopes beside the existing
SDK-supervisor completion record. Driver teardown removes all pending entry
work and application graphs inside the SDK task. Actual supervisor join then
selects a reachable poisoned record containing closed connection metadata,
original tickets and physical owners; it cannot release those as successful
drain. Recovery neither polls application work nor destroys its observers.
Successful socket/entry disposal releases its original owners; pending native
workers retain their existing independent actual-join requirement. This adds
no connection task, admission namespace or recovery pool, and scoped expiry
does not gain a fresh cleanup wait.

Official native socket ports remember a received peer Close or stream end,
drain the upstream automatic reply, and finalize that native state idempotently.
Only close finalization treats an already closed handshake as completed.
Ordinary read/write misuse and passive write-observation invariants remain
infrastructure failures. Closing does not release the current physical text
owner before actual socket/buffer destruction.

A managed WebSocket source task retains its setup and event senders outside
application phase futures. The existing task guard records callback or disposal
failure before either port can close and wake an outer receiver. Setup borrows
its sender and preserves the normal Ready/Response close point; event execution
uses the same guarded boundary. This keeps the task's failure report authoritative
when a receiver concurrently observes closure, without another task or report.

Closing an unaccepted validation handoff cannot claim its private observation.
The original guarded SDK task still owns failure reporting through future and
panic-payload destruction. After bounded acceptance, the SDK receiver owns
rejected-output disposal and its associated reporting before original worker
completion or retirement. Both paths contain observer destruction and retain
the existing shared one-attempt claim and poisoned-worker evidence.

Process recovery visits a finite snapshot of retired records in each existing
endpoint registration before entering its epoch wait. Existing registration
and generation cursors retain bounded traversal; restoring Pending joins cannot
extend the pass. A ready record behind another join therefore needs no unrelated
notification to be polled. Native polling and destruction remain outside table
locks, and only actual join releases original physical ownership. This adds no
queue, task, admission budget or repeated wake for an all-Pending table.

Optional `shutdown_on(signal, options)` hooks use the server's standard method,
not a server extension trait, `graphql.run`, separate result await or manual
driver/guard/reaper sequence. Immediate closes GraphQL admission and requests
cancellation. Graceful defaults to 30 seconds, supports a valid typed override,
and waits for finite SDK execution and already processing/sending events, not
new source items or the unstarted queue. One monotonic deadline covers SDK
drain and active cleanup, with existing shorter limits still effective.

Managed HTTP elects finite eligibility after operation/media selection and
before finite context construction, under the same control gate as shutdown.
Requests still receiving, decoding or resolving an unselected operation cancel
on Graceful without new context/source work. A prepared finite rejection can
elect eligibility before notification as well. The resource-free marker shares
existing HTTP deadline metadata; cancellation retires the same original worker,
request permit and clocks through actual join or poisoned recovery. It does not
make unselected input eligible by merely admitting the native HTTP request.

The hook drains the SDK first and then signals native server shutdown. TCP
admission and other routes may continue during SDK drain; its duration is not
a bound on the server's later shutdown or proof of peer receipt. Without a
hook, last service-owner release requests cancellation but there is no
coordinated drain guarantee or automatic detection of shutdown start. A native
server can therefore wait for long-lived subscriptions if it never notifies
the SDK. Neither mode preempts blocking application code or exits the process.
Keep timeout/failure results observable through safe typed optional inspection,
physical charges until actual cleanup, and independent endpoint/runtime scopes.
Process-global recovery remains SDK-internal, not a required application call.

Mock finite HTTP correlation remains application-owned across SDK task handoff.
The native application carries a bounded `TraceId` derived from its existing
numeric request ID. A fixed weak scope index, bounded by the existing HTTP
request inventory, reconnects finite context construction and existing observers
to the original listener-bound scope. The same HTTP admission permit follows
that scope through application context destruction. This adds no admission
budget or future/task registry and does not encode request values in metadata.
Streaming setup and event phases retain their separate fresh factory scopes;
ordinary non-managed finite handlers may continue using their ambient scope.

Mock fixes its existing five-second application deadline at the first shutdown
request and immediately notifies the writer of that deadline. The managed SDK
drains GraphQL within the original HTTP cutoff, retaining the existing 500-ms
reserve for VM/writer recovery. Native connection shutdown and VM cancellation
follow the SDK drain, expiry or classified failure. This replaces early VM
cancellation and manual endpoint/process drivers. Native connection cutoff,
actual task destruction and subsequent VM/writer stop reuse the same absolute
deadline; they do not receive another cleanup interval.

Document complete profile-encoded event sizing, per-subscription queued plus
in-flight bytes, maximum pre-demand reservations, and shared control/setup
headroom separately from arbitrary domain/network memory. Add a compact,
maintained Axum finite-Query/POST-SSE application with ordinary routing,
admission, automatic supervision, disconnect cancellation, no-hook fallback
and standard-hook shutdown. Explain source/event/field resolver roles and cheap
shared domain backing. Show compatible consumer and standalone generated-crate
workspaces and producer identity/ABI checks.

Preserve ABI 8 and all Mock/CLI formats, resource policy, protocol profiles and
application replay/ACK responsibilities. Package metadata generation remains
separate. Handoff Section 42 and Conformance Section 42 define the focused
local implementation and acceptance boundaries; no product work is claimed by
publishing this design.

## 51. Distributed tracing context propagation

[ADR-0056](adr/0056-distributed-tracing-context-propagation.md) is Accepted /
Implemented with focused local macOS acceptance. It amends ADR-0012's
correlation-only tracing boundary without moving
backend types into the core/facade or generated signatures. Existing
`OpenTelemetryObserver` and `TracingObserver` remain event-only paths; their
internal span relationships do not prove incoming parent adoption or current
context activation during resolver execution.

Add one optional `observability().instrumentation(provider)` slot to the same
component graph. The official `OpenTelemetryInstrumentation::new(tracer)`
belongs in `chigiri_opentelemetry`; default consumers gain no tracing/OTel
dependency. Capture a valid current span context before SDK task handoff and
request-start observation, otherwise extract bounded W3C headers, otherwise
create a new trace when recording is enabled. Opaque `TraceId` is not a parent.
An effective SDK context supplies the actual telemetry trace ID.

Carry the standard OpenTelemetry suppression boolean explicitly alongside the
bounded SpanContext, even when no valid span exists. Capture it in the owning
execution before parent selection and SDK handoff; header fallback and optional
correlation loss never clear it. Do not copy arbitrary Context values or baggage
to preserve this control. It is not a wire field, sampling decision or setting.

SDK-owned scopes activate the appropriate context for synchronous invocation,
each async poll, and owned cancellation/destruction, restoring ambient context
and suppression on return or unwind. Nested owning suppression scopes follow
child handoffs; unrelated worker ambient values do not become request state.
Cover direct execution, HTTP, providers, context factories, resolvers and both
official bridges, with no adapter-local tracing policy.
Applications access `opentelemetry::Context::current()` and standard propagators.
A compatible context-activating tracing layer connects ordinary contextual
application spans. Per-field scopes remain opt-in; disabled instrumentation
adds no per-field work. No context span field or request plugin is required.

Subscription setup closes promptly. Each admitted event gets a fresh root
trace with a creation-time link to the bounded setup SpanContext when recording
is enabled and not suppressed; never inherit publisher or previous-event
context, keep a live setup span, or reuse setup application context.
Every event retains setup's separate suppression control,
including with an invalid parent; a fresh scope cannot re-enable telemetry.
Event-local suppression does not modify later events' setup baseline. Existing
metadata accounting includes retained trace state. Optional tracing capacity
failure degrades telemetry rather than
changing otherwise admissible GraphQL work. All three transport profiles use
this same policy without new protocol payloads or SDL fields.

Preserve fixed Chigiri lifecycle sampling, independent provider sampling,
always-delivered error-observer events, redaction, physical cleanup and SDK
shutdown semantics. Standard suppression prevents SDK OTel/tracing span emission,
including error conversion, but does not suppress independent observer delivery
or GraphQL processing. Capacity degradation cannot clear its fixed control bit.
A dropped lifecycle trace may still propagate its adopted parent; it does not
force sampled roots or exporter success. Baggage and
arbitrary OpenTelemetry context values are not automatically carried. Invalid
trace headers do not add application errors or bypass existing admission limits.

Applications own provider/exporter/subscriber configuration and shutdown,
downstream client injection, and explicit propagation to user-created tasks.
The SDK neither installs global telemetry nor blocks for export. Keep generated
ABI 8, event contracts and Mock/CLI formats. Migrate built-in observer-based
tracing to one provider without automatically recording the same spans twice.
Handoff Section 43 and Conformance Section 43 specify focused local obligations;
publication is not implementation or acceptance evidence. The
[integration guide](distributed-tracing.md) describes the current public route;
focused evidence identifies executed local
checks and their limits. Source-bound independent acceptance is recorded in
Beads `lab01-ija.9` and `lab01-ija.10`; the original baseline characterization
remains a separate open follow-up without a fix claim.

## 52. Typed Mock event sources and transactional consumers

[ADR-0057](adr/0057-typed-mock-event-sources-and-consumers.md) is Accepted /
Implemented with focused local macOS acceptance. It amends the Mock
source/publication portion of Section 49 through the coordinated clean cutover.
Current evidence is recorded in the validation record;
both bounded independent reviews accepted the implementation. Earlier
ADR-0054/0055/0056 evidence retains its original source scope.

Use adopted Subscription output named types as public channel identities.
Generate `ctx.pubsub.subscribe.<Type>`, `ctx.pubsub.publish.<Type>` and
`ctx.generator.sequence.<Type>` without field-name aliases. Query references
do not affect eligibility. Capture a publication once and deliver once to each
compatible same-type destination, preserving each field's list/nullability,
constraints and provenance. Different named types remain isolated. Setup
selectors retain destination-owned exact payload/source contracts and host
permission/provenance checks. GraphQL setup returns its own field's opaque
source. Configured server-lifetime consumers return a source from `subscribe`
and run `consume(ctx, event)` in a fresh short VM for each item. No arbitrary
closure, suspended VM, writable context or publisher credential is retained.

Sequences actively generate from captured finite datasets at monotonic intervals
independent of consumption, starting after the first interval. Each subscription
has its own origin/position; missed ticks skip positions without catch-up work.
Timeleap affects event apparent time, not generation cadence. Sequences feed
their direct consumer; bridging to pub/sub requires explicit publication.

Each named consumer has its own atomic model-write/publication candidate.
Ordinary failure rolls it back, records the result and continues without retry.
Root events have depth 0; republish edges 1 through 3 are permitted and a depth-4
attempt fails the complete candidate even through `pcall`. Depth is immutable
host authority. Existing six hooks and GraphQL event resolvers gain no writes.

Share source-stage admission, bounded queues/cursors, timer/cancellation owners
and overflow policy across source/consumer kinds. Keep captured source budgets
distinct from existing encoded GraphQL output budgets. Default non-dropping
source overflow ends only the affected consumer; explicit DropOldest evicts its
eligible unprocessed items. Combine actual source/encoded evictions without
double counting and omit missed ticks from delivery-loss metadata. All-topic
selection is explicit and limited to named consumers.

One Mock Worker owns and physically joins both its synchronous VM scheduler
and a fixed captured-delivery driver. Delivery and source expiry continue while
a VM callback runs; the driver retains no VM, request or writable candidate.
GraphQL registration finalization activates an independent monotonic sequence
origin and wakes that driver. Named startup finalizes all selected registrations
before activating any origin or admitting HTTP traffic. The same VM scheduler
round-robins one ready named input between ordinary FIFO jobs. It obtains the
existing write permit without waiting before dequeuing; a GraphQL candidate
that needs the VM to release its permit therefore cannot deadlock the worker.
Waiting inputs remain evictable and drop counters are sampled only at that
invocation-start boundary. A bounded wake channel carries readiness only,
never an event or execution context. Direct sequence entries share source
admission and queue policy, while remaining outside pub/sub topic matching and staging
pressure. Timer visits select at most one latest due value; finite producer
completion allows accepted input to drain.
Dispatch pressure remains active through actual batch release, even after
cursors advance, and follows the source policy of actual publication retainers.
Idle sources are not recovery targets and dispatch bounds do not increase.

Register consumers explicitly in `mock.luau`; local `mock codegen` creates types
and absent editable module scaffolds. Preserve adopted-schema authority,
existing sandbox/worker/Record rules and SDK-owned physical cleanup. Graceful
finishes already-running work within its deadline, not entire queued event
chains. An event-start fence orders Graceful selection against named invocation
admission. Graceful stops new generation and callbacks, lets only the begun
candidate finish within the original deadline, and discards waiting work without
executing publication chains. The native cancellation/join follows that drain;
Immediate retains cancellation of the running candidate. Fresh init replaces
compatibility readers, aliases and old-project update exceptions. The ADR
specifies the coordinated current versions and finite limits; Handoff
Section 44 and Conformance Section 44 define work and acceptance.
Rust ABI 8, default dependencies, transport profiles and tracing remain intact.

## 53. Tiered validation and weekly qualification

[ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md) supersedes the
universal full-suite and per-release long-run frequency in earlier sections.
Ordinary local/CI validation defaults to the fixed-toolchain `ci` profile on
both native Tier 1 platforms. The explicit `full` profile preserves the former
complete catalog, including current stable and broad optimized/large-consumer
coverage. Optimized Mock safety regressions remain in ordinary CI.

Weekly CI runs `full` on both Tier 1 platforms, six 30-minute Linux fuzz targets
and isolated formal load/15-minute soak. Relevant high-risk changes add their
long-run coverage; first qualification and major releases retain the complete
campaign. Controlled comparative benchmarks keep their source/binary/hardware
and human-baseline requirements. GraphQL, privacy, bounds, physical ownership,
platform support and generated contracts are unchanged.

Completion uses `chigiri/product-validation-v2` with an explicit profile, source
digest and exact ordered results. Required CI receipts bind both native success
results to `ci`; ordinary evidence cannot satisfy `full`. See the
[validation guide](validation.md), Handoff Section 45 and Conformance Section 45.

## 54. Craftguild standards integration

[ADR-0059](adr/0059-craftguild-standards-adoption.md) defines the common
integration boundary and takes precedence over the earlier physical-line,
test-inclusive and native `make release` command descriptions. `make build`
is the native optimized build; release preparation is a separate operation.

The CLI owns common presentation and an embedded CycloneDX artifact; normal
SDK libraries and GeneratorInfo formats remain independent. Build-time shared
verification seals actual source, lockfiles, compiler, target, profile, features
and embedded assets. Engine v0.2.0 prepares debug/release identity directly and
owns compilation and receipts; no repository adapter rewrites prepared records.
No build script recursively invokes Cargo. The shared engine also owns
conditional AST measurement. ADR-0059 records source-bound product validation
receipt reuse and the secure distribution packaging boundary. Shared build-tool
regressions belong in standards; this repository retains actual builds, artifact
verification and product behavior checks without duplicate build-test suites.

Shared quality policies retain product-owned ownership, exact generation,
English, host, MSRV, generated consumer and resource checks. The shared conditional
Rust separator changes only temporary counting input, with unchanged source
lines; it cannot make an unsupported measurement pass. Compiled GraphiQL
payloads are regenerated and licensed while authored glue is measured at source.
Git and immutable technical receipts describe build/release provenance. No
business lifecycle, state history, schema or Header-Detail relationship changes.
