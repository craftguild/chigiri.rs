# ADR-0053: Trait-only resolvers and application boundaries

Status: Accepted

Date: 2026-10-17

Implementation: Implemented. RI-WP0 through RI-WP3 and RI-01 through RI-08
have passed focused local macOS arm64 validation, including MSRV/stable lanes
and both independent reviews. See the
validation record and
Beads review `lab01-c43.10`.

Generated ABI target: 7

Implementation goal: Prompt for a new agent.

## 1. Context and decision

Application-integration experiments exposed two kinds of work:
connecting domain data to an SDL contract, and learning Chigiri-specific ways
to express that connection. Its GraphQL adapter uses `SchemaTypes` to retain
domain objects, explicit projections, application-error conversion, and a
per-request context factory. The consumer requested less mechanical work while
preserving the typed API boundary.

The experiment's REST adapter forwards existing RPC JSON, while its GraphQL
adapter establishes another typed contract. Their source-line counts do not
measure equivalent guarantees. Typed REST responses also need conversion when
their public shape differs from a domain or ORM model. Final serialization is
compatible with preserving Rust types through application code.

Generating another output DTO would not remove conversion between independent
Rust types. Requiring domain code to return that DTO would move API dependencies
and conversion responsibilities into the domain. Existing associated-type
binding already retains domain values and resolves their selected fields
without constructing another output object.

Projection builders shorten getters, but add `project_*`/`resolve_*`
terminology, field-order typestate, and a choice of implementation forms.
Scaffolding helps initial entry; it does not synchronize handwritten mapping
with later SDL changes. Automatic field binding would introduce more conventions.

Use complete generated resolver traits as the only Rust field implementation
form. Keep domain mapping explicit in ordinary Rust, centralize application
error conversion, and document the shortest existing context-factory path.
Prefer familiar language and IDE operations over the smallest source count.

## 2. Supersession and preserved boundaries

These replacements take effect together at implementation cutover:

| Earlier decision | Replacement |
| --- | --- |
| [ADR-0018](0018-projection-resolvers-and-generated-surface.md) | Remove projection plans, builders, facade, field-stage machinery, and the projection-specific `ResolverFuture` alias. Complete resolver traits become the sole ordinary implementation form. Preserve typed dispatch and execution policy. |
| [ADR-0020](0020-explicit-projection-scaffolding.md) | Retire the scaffold command, public library API, and dedicated acceptance requirements. Do not replace them with trait scaffolding. |
| [ADR-0021](0021-structured-diagnostics-and-doctor.md) and [ADR-0024](0024-large-consumer-dx-conformance.md) | Replace projection/scaffold guidance and acceptance with trait implementation and application-error coverage. Preserve doctor, original rustc diagnostics, redaction, and applicable resource/large-consumer checks. |
| [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md), [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md), and [ADR-0028](0028-semantic-generated-layout-and-handwritten-line-budget.md) | Target ABI 7 and remove projection output units. Preserve the standalone generated crate, ownership, exact checks, transactions, producer identity, semantic layout, and handwritten limits. |
| [ADR-0048](0048-chigiri-brand-and-single-binary-cli.md) | Remove only `chigiri rust scaffold` from the public command tree. Preserve the single binary and remaining Rust/Mock namespaces. |

Retain [ADR-0002](0002-cli-codegen-and-resolvers.md)'s complete resolver
contracts and RPITIT/`async fn` model. Retain
[ADR-0005](0005-graphql-rust-type-mapping.md)'s domain associated types, input
presence, enums, interfaces/unions, and scalar contracts. Change the generated
resolver's application error position below, not core `FieldError` or
`FieldResult` semantics.

Preserve the endpoint, application-owned server lifecycle, middleware,
authorization, selection, coercion, null propagation, concurrency, cancellation,
limits, and observability. Mock project, bundle, host API, reference, and
validation formats remain unchanged.

No compatibility alias, optional projection feature, ABI 6 execution adapter,
or parallel authoring mode survives cutover. Previous validation records remain
evidence only for their recorded source; their passes do not prove this ADR.

## 3. Generated contract and explicit mapping

### 3.1 Resolver traits

Generate one complete resolver trait per object, including operation roots.
Add a required error parameter:

```rust,ignore
pub trait UserResolver<C, T, E>: Send + Sync + 'static
where
    C: Send + Sync + 'static,
    T: SchemaTypes,
    E: std::error::Error + Send + Sync + 'static,
{
    fn name<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, C>,
        parent: &'a T::User,
    ) -> impl std::future::Future<Output = Result<String, E>> + Send + 'a;
}
```

Every SDL field is a required method without a default body. Implementations
may use ordinary `async fn`. Preserve existing parent, argument, naming,
borrowing, and output rules, including `Option<&T::Root>` when a root also
occurs as a nested object. Methods need not follow SDL order in an `impl`.

All groups in one resolver assembly use the same `E`; different schemas may
use different errors. This parameter does not add an associated error type to
`SchemaTypes` or reserve SDL names such as `Error`. Do not supply a default
parameter that silently chooses another error policy.

The generated crate remains independent of application/domain crates. Generate
traits, adapters, `SchemaTypes`, input/argument types, enums, abstract-type
wrappers, assembly, and the static descriptor. Add no output-object DTO,
binding macro, derive, reflection, implicit `Into`, getter trait, or inspection
of domain source.

### 3.2 Application example

For this SDL, the application binds `User` to `domain::User`:

```graphql
type Query {
  user(id: ID!): User!
}

type User {
  name: String!
  displayName: String!
}
```

This is the target API, not an ABI 6 example. The application supplies
`AppContext`, its typed user service, and the domain types. Acceptance must
compile this pattern in a real consumer.

```rust,ignore
struct Types;

impl generated::SchemaTypes for Types {
    type User = domain::User;
}

struct QueryFields;

impl generated::QueryResolver<AppContext, Types, domain::AppError> for QueryFields {
    async fn user<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        args: generated::QueryUserArgs,
    ) -> Result<domain::User, domain::AppError> {
        ctx.context().users.find(args.id.as_str()).await
    }
}

struct UserFields;

impl generated::UserResolver<AppContext, Types, domain::AppError> for UserFields {
    async fn name<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
        user: &'a domain::User,
    ) -> Result<String, domain::AppError> {
        Ok(user.name.clone())
    }

    async fn display_name<'a>(
        &'a self,
        _ctx: chigiri::FieldContext<'a, AppContext>,
        user: &'a domain::User,
    ) -> Result<String, domain::AppError> {
        Ok(format!("{} {}", user.surname, user.given_name))
    }
}
```

Map selected GraphQL fields onto the retained domain value. A domain method
may provide the calculated value; its placement is an application decision.
I/O, fallible conversion, and context-dependent fields use the same trait form.
No intermediate output DTO or JSON round trip is required.

Compilation checks required methods and output shapes. The application owns
calculation semantics and disclosure policy. Private members need normal
accessors, not a generator exception to Rust visibility.

### 3.3 IDE workflow

Use ordinary `impl` blocks and generated trait declarations with SDL
descriptions. Required-member insertion, body completion, and navigation need
no Chigiri editor plugin or DSL. Keep private adapters hidden.

Compilation alone does not prove IDE behavior. Record editor/rust-analyzer
versions for an actual smoke check, or mark it unexecuted. That manual check
supplements automated acceptance and is not a blocker when tooling is
unavailable; do not claim verification or add another generator mode instead.

## 4. One application-error conversion

### 4.1 Assembly API

Register one synchronous mapper when starting resolver assembly:

```rust,ignore
fn map_app_error(error: domain::AppError) -> chigiri::FieldError {
    match error {
        domain::AppError::NotFound => {
            chigiri::FieldError::public("User was not found").code("NOT_FOUND")
        }
        other => chigiri::FieldError::internal(other),
    }
}

let resolvers = generated::Resolvers::<AppContext, Types>::builder(map_app_error)
    .with_query(QueryFields)
    .with_user(UserFields)
    .build();
```

`builder<E, F>(mapper: F)` requires
`F: Fn(E) -> chigiri::FieldError + Send + Sync + 'static` and Section 3's error
bound. Each existing `with_*` stage requires its group's trait with that same
`E`. Retain schema-order group registration and compile-time completeness;
only the final stage exposes `build`.

Infer `E` from the mapper. The finished public type remains `Resolvers<C, T>`.
Private group adapters retain the mapper and erase application errors before
runtime dispatch. Add no application-error registry or parameter to the core
executor or endpoint builder.

The mapper is mandatory. Its absence or an incompatible mapper/group error
type is a compile failure. An application selecting `FieldError` supplies
`std::convert::identity::<chigiri::FieldError>`. An infallible application may
select `std::convert::Infallible` and an exhaustive empty match.

Ordinary `?` works for failures already typed as `E` or implementing Rust's
`From` conversion into `E`; no transitive conversion is invented. An external
domain error works through an API-owned mapping function without a foreign
trait implementation or a Chigiri dependency in the domain.

### 4.2 Execution and disclosure

Invoke the mapper exactly once per selected resolver's returned `Err(E)`, after
its future completes and before ordinary field-error handling. Successful and
unselected fields do not invoke it. Retain the application-panic boundary; the
mapper is not an async I/O hook.

Applications explicitly select public messages, codes, and extensions.
Examples use a public allowlist and `FieldError::internal` for other failures,
retaining their source. Never automatically expose `Display`, `Debug`, or
serialized error contents.

Preserve aliases, locations, paths, partial data, null bubbling, error IDs,
observation, and HTTP status behavior. Mapping does not bypass field policy.

Framework argument conversion, scalar/enum conversion, output completion,
middleware, introspection, and invariant failures retain their `FieldError`
handling and never pass through this mapper. Keep list-item completion errors
and index paths. Introduce no `Vec<Result<_, E>>` mapping or other change to the
existing output type grammar.

## 5. Context through existing APIs

Use `Endpoint::builder(...).context_factory(...)`, the existing closure/function
implementation of `ContextFactory`, and `PreparedContext::execution_only`.
Add no context macro, factory generator, new context trait, or endpoint entry
point.

The standard recipe must construct a fresh context per request, including a
closure capturing shared services through `Arc`. Share pools/services
deliberately, without reusing request identity, caches, or session state across
requests. Show the required boxed-future signature and request lifetime.

A named function can express the lifetime directly:

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

`AppContext::prepare` is application-owned and returns `PrepareError`, not a
new Chigiri method. Canonical examples must compile both this pattern and the
captured-service closure against the actual public API.

Keep `PrepareError::Reject` for explicit transport rejection and
`PrepareError::Internal` for masked preparation failure. Preserve preparation
timing, response hooks, deadlines, drop/cancellation, and the advanced
`PreparedContext<C, S>` path. The resolver mapper does not reclassify
preparation failures as field errors.

## 6. CLI, migration, and versions

Remove `rust scaffold` grammar, help, dispatch, `scaffold_projection`, and
scaffold-only inspection/rendering APIs. Do not remove shared generation or
ownership machinery merely because its internal name contains `scaffold`.
There is no command alias or replacement trait scaffold.

Retire scaffold-only `SF4014` and `SF4020` uses without recycling their codes.
Keep diagnostic formats and applicable `SF4018`/`SF4019` meanings for correlated
resolver guidance. Doctor preserves rustc records and adds only supported
guidance; remove suggestions to call deleted APIs.

Cut over ABI 6 to 7 in guards, descriptors, generation, and maintained consumers
together. Update producer/inventory hashes normally. Do not change manifest,
ownership-marker, generator-info, or Mock format shapes solely for this change.
Preserve dependency/MSRV boundaries and generation resource limits.

Use canonical `rust update` for an existing verified generated crate and
`rust codegen` for a new destination. Retain prior inventory/ownership checks.
Reading retained-format provenance to replace owned output does not authorize
executing ABI 6 or retaining its renderer. Generation must not require the old
application implementation to compile first. Reject modified, unknown, or
mixed owned output instead of overwriting handwritten data.

Migrate maintained applications to complete traits, register their mapper, and
regenerate canonical output. Remove projection-only fixtures or replace their
still-applicable safety coverage with trait fixtures. Preserve unrelated tests
and historical evidence. Update quick start, migration, doctor guidance,
large-consumer checks, inventories, and affected validation commands together.

The workflow is SDL edit, contract regeneration, consumer compiler diagnostics,
and handwritten correction. Add no application-source merge. Adding fields or
objects remains a compiler-enforced obligation.

## 7. Acceptance and validation

These are normative requirements. RI-01 through RI-08 passed within the local
scope recorded in the validation evidence;
the table defines the required assertions rather than replacing that evidence:

| ID | Required evidence |
| --- | --- |
| RI-01 | Public/generated source and real CLI help expose only complete resolver traits for field implementation. Projection builders, setters, `ResolverFuture`, and scaffold APIs are absent; `rust scaffold` is rejected without writes. |
| RI-02 | Separate consumer/domain crates compile direct domain returns, normal/calculated fields, nested objects/lists, and existing interface/union/scalar cases. The domain has no Chigiri dependency/attributes; no output DTO or JSON round trip is required. |
| RI-03 | Compile-negative consumers reject missing fields/groups, wrong parent/output types and nullability, a different group error type, a wrong mapper, and a missing mapper. Canonical regeneration exposes SDL additions. |
| RI-04 | Query, Mutation, and object fields share an external application error and mapper with ordinary `?`. Direct and real HTTP execution preserve public details, internal masking/source observation, alias paths, partial data, and bubbling. A counting mapper runs once per resolver failure and never for successful/unselected fields. |
| RI-05 | Conversion, completion, middleware, and list-item failures retain framework handling without remapping. Bubbling does not duplicate error IDs or observation; an identity mapper supports `FieldError`. |
| RI-06 | Actual API checks compile named factories and captured-`Arc` closures with `execution_only`. HTTP evidence proves request-local context/cache/session isolation, deliberate service sharing, and preserved preparation rejection, internal failure, timeout/drop, and response-hook behavior. |
| RI-07 | ABI 6 output fails the ABI guard against ABI 7. Canonical update of verified old output and fresh generation succeed, with modified output protected. Exact ownership, semantic layout, deterministic generation, and applicable large-consumer/resource checks pass after regeneration. |
| RI-08 | Maintained documentation/examples use traits and the common mapper. Automated compilation checks documented signatures. Record actual IDE body completion, required-member insertion, and navigation, or explicitly mark them unexecuted. |

Map these IDs to focused tests and retained coverage in the
[conformance matrix](../spec-conformance.md#40-trait-only-rust-integration-conformance).
Follow the [implementation handoff](../implementation-handoff.md#40-trait-only-rust-integration-work-packages).
Track new reviews in Beads under the [review workflow](../review-workflow.md).
Previously completed reviews are in the Beads review linked above.

Implementation validation uses local macOS arm64 and available MSRV/stable
toolchains. Run affected generator/CLI, consumer, runtime, HTTP, canonical exact,
formatting, Clippy, rustdoc, and implementation-rules checks. Group compatible
tests and use existing subprocess bounds. No mandatory full-product validator,
remote Linux, cross-build, publishing, or load/fuzz campaign is added. Record
unexecuted environments without changing support tiers or reusing pre-change
artifacts as proof.

## 8. Consequences and exclusions

Ordinary trait implementations are longer than simple projection chains, but
there is one implementation model without a field-stage DSL. Mapping stays
visible, normal IDE operations apply, and API dependencies stay out of the
domain. One mapper removes repeated error conversion while preserving GraphQL
error handling.

This decision promises no source-line or authoring-speed reduction. Measure
such claims against equivalent contracts. Record any invalidated runtime
performance evidence separately.

Subscriptions/stream transports, ORM integration, DataLoader APIs, output DTOs,
automatic binding, borrowed output objects, RPC changes, scalar representation
changes, and Mock authoring changes are outside this ADR. Typed application/RPC
clients are useful consumer work, separate from changes to another
application in this assignment.
