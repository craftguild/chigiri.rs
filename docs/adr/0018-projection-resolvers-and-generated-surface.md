# ADR-0018: Reduce implementation volume with projection resolvers and isolate the generated surface

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md)
> and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> define the next target for generated crates, the CLI, configuration, ABI, and
> distribution boundaries (Accepted / Planned). They replace only the relevant
> parts of this decision; existing GraphQL and security contracts and historical
> implementation evidence remain valid.

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0002, ADR-0005, ADR-0016, ADR-0017
- Coordinates: ADR-0020, ADR-0021, ADR-0024
- Generated ABI target: 5

> 2026-10-17 implemented supersession:
> [ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) removes
> projection plans, builders, setters, and the projection-specific future alias
> at the ABI 7 cutover. Complete resolver traits become the only field
> implementation form, with one application-error mapper. This ADR's
> implementation history remains valid; local replacement evidence is recorded
> in the ADR-0053 validation record.

## 1. Context

A complete schema-first resolver trait detects drift between the SDL and its
implementation at compile time. In a large schema, however, even a simple field
projection from a domain object requires an async resolver-trait implementation.
Generated code and application boilerplate can overwhelm users.

Reducing generated code by returning to runtime reflection, string-keyed field
maps, or null fallbacks for missing resolvers would sacrifice Pythia's safety.
The goal is to preserve complete generation while hiding private mechanisms
from ordinary users and providing a concise, type-safe form for simple
projections.

## 2. Decision

### 2.1 Two implementation forms, one resolver assembly

A non-root object field group accepts two forms:

1. A complete generated resolver trait equivalent to the existing form. This is
   the deep SPI for full control over async execution, context, arguments,
   errors, and custom completion.
2. A generated projection plan. This is the ordinary path: register simple
   fields with synchronous projections and only the necessary fields with full
   resolver closures.

Both forms are passed to the same generated
`Resolvers::builder().with_user(...)`. Neither the endpoint nor the schema gains
a separate advanced entry point.

```rust,ignore
let users = generated::projection::User::<AppContext, Types>::builder()
    .project_id(|user| user.id.clone())
    .project_display_name(|user| user.display_name.clone())
    .resolve_friends(|ctx, user, args| Box::pin(async move {
        Ok(ctx.load_friends(user.id.clone(), args.first).await)
    }))
    .build();

let resolvers = generated::Resolvers::builder()
    .with_query(QueryFields)
    .with_user(users)
    .build();
```

Query, Mutation, and Subscription root objects are not eligible for projection;
they require the complete resolver trait. Root fields are boundaries for
authentication, I/O, side effects, and operation errors and must not share the
automatic defaults of domain-member projections.

### 2.2 Projection eligibility and signatures

Only non-root fields without arguments receive a `project_<field>` method. Its
closure has this conceptual contract:

```rust,ignore
Fn(&Types::User) -> GeneratedExactOutput + Send + Sync + 'static
```

Fields that need context, field arguments, async work, or failure use
`resolve_<field>`. Its inputs, `FieldResult`, path, null bubbling, and diagnostics
match the same field method on the complete resolver trait. The complete trait
returns `impl Future`; the stored resolver closure returns the public
`pythia::ResolverFuture` alias, so users construct the future with
`Box::pin(async move { ... })`.

`project_*` performs no implicit `Clone`, `Into`, string parsing, null fallback,
or error swallowing. When an owned value is required, the closure performs the
clone or conversion explicitly. Its return value must be the exact Rust type
generated from the SDL nullability, list, scalar, and domain binding, or the
application does not compile.

There is no default field-name reflection or automatic search for a matching
domain field. A closure remains mandatory even when SDL and Rust field names
match. ADR-0020 retains this rule and helps users author the initial explicit
closures with a stdout scaffold. No derive or third resolver form is added.

### 2.3 Compile-time completeness

The projection builder is a linear typestate in SDL source order. At each stage,
IDE completion offers only `project_*` when eligible and `resolve_*` for the
current field. Calling either advances to the next stage. `build()` is available
only after the final field.

- Missing fields, duplicate registrations, out-of-order registrations, and
  incorrect return types are compile failures.
- No runtime string field registry, publicly constructible partial plan, or
  missing/null fallback is used. Private slots may hold `Option<Resolver>`, but
  each setter fills the exact slot in source order, and only the final const
  stage implements `build`. A valid public builder chain cannot reach a missing
  branch; an internal invariant failure is a generator bug.
- Private indexed dispatch and type-erasure adapters are permitted, but setters
  establish their types and runtime dispatch has no “resolver missing” branch.
- Each closure is evaluated once while building, not rebuilt for every request.

Complete trait implementations and projection plans both convert to a generated
sealed input, preventing consumers from implementing an incomplete third form.

### 2.4 Generated source isolation

Checked-in generation and compile-time coverage of every field remain. The
volume needed for safety is accepted, while the normal user-facing surface is
limited to:

- `SchemaTypes` and wire, input, and domain-binding types.
- Complete resolver traits.
- The `Resolvers` assembly.
- One `projection::<Object>` entry per object and its documented
  `projection::<object>::Builder<C, T, INDEX>`.
- The schema descriptor and ABI-mismatch diagnostic.

`Builder` is one documented public type carrying a const stage. IDE completion
shows only the inherent methods for that stage. Slots, plans, adapters, indexed
dispatch, and fingerprint helpers are private or `#[doc(hidden)]`. Generated
output is sharded beneath `generated/mod.rs` and `generated/__pythia/`; shard
names and file layout are not public API. Hundreds of stage types do not appear
at the rustdoc root or in ordinary IDE completion.

Every generated `.rs` file also remains within 400 physical lines. Stage
responsibilities are not hidden in giant macros, minified output, or giant
strings. Generator drift checks atomically inspect the whole generated
directory, including private shards.

### 2.5 ABI transition

Introducing sealed group inputs, projection dispatch, and the generated module
layout raises generated ABI 4 to ABI 5 in one change. Runtime, generator,
goldens, books, Axum adoption, the custom-scalar fixture, and the large consumer
are regenerated together.

Because the project has not reached a production release, it adds no ABI 4
reader, adapter for old generated sources, dual-ABI feature, or deprecation
shim. An ABI mismatch is a compile-time diagnostic that includes the regeneration
command.

### 2.6 Resource and execution semantics

Projection results pass through the same completion, custom-scalar governance,
list/node/byte limits, error masking, and observers as ordinary resolvers. A
projection is not a bypass for cache, cost, authorization, plugins, or field
instrumentation.

Projection closures are synchronous and must not block. I/O and long-running
work use `resolve_*`. A panic in either a projection or complete resolver crosses
the same application-failure boundary; core does not catch it with
`catch_unwind` or convert it to a GraphQL error. If the process uses
`panic=abort`, it can terminate, so applications keep resolvers panic-free.

## 3. Acceptance matrix

| ID | Expected |
| --- | --- |
| PROJ-01 | A non-root field without arguments receives `project_*` and `resolve_*`; a field with arguments receives only `resolve_*`. |
| PROJ-02 | Root objects generate no projection and require a complete resolver trait. |
| PROJ-03 | A projection plan covering every field produces the same response, path, and null bubbling as a complete trait implementation. |
| PROJ-04 | Projection and async/error resolvers can coexist in one object plan. |
| PROJ-05 | A missing field, duplicate, order violation, or incorrect output is a compile failure with a stable diagnostic fragment. |
| PROJ-06 | Generated and runtime sources contain no runtime field-name map, reflection, public partial registration, or missing/null fallback. |
| PROJ-07 | Projection does not bypass plugins, cost, limits, observers, or custom-scalar completion. |
| PROJ-08 | The complete resolver trait remains the deep SPI and assembles through the same `with_<object>` method. |
| PROJ-09 | Public rustdoc shows only the entry and one `Builder`; private slots, plans, shards, and helpers remain hidden, and IDE candidates at each stage are limited to the current field. |
| PROJ-10 | The CLI checks the whole generated directory for drift and replaces it atomically. |
| PROJ-11 | Every generated file is at most 400 lines, and the complete large consumer is at most 64 MiB. |
| PROJ-12 | A 70-object, 750-field consumer passes fmt, check, test, Clippy, and rustdoc on MSRV and stable within 180 seconds per command. |
| PROJ-13 | Checked-in output uses only ABI 5 and contains no ABI 4 runtime adapter or shim; a negative test creates mismatched output temporarily. |
| PROJ-14 | The books quick start uses projections to reduce boilerplate while retaining Query, Mutation, GET, and diagnostics. |

## 4. Consequences

### Positive

- Simple domain-object fields require less code without sacrificing complete,
  type-safe coverage.
- The volume of generated mechanisms is separated from the surface seen by
  applications.
- Deep SPI control can be selected only for fields and objects that need it.

### Negative

- The generator and ABI change substantially, and more compile-negative fixtures
  are required.
- Adding an SDL field still requires a matching closure because of linear
  typestate completeness.
- Explicit cloning in projection closures is slightly verbose but keeps
  ownership and cost visible.

## 5. Rejected alternatives

- Resolve domain field names automatically through reflection or derive:
  schema/domain drift and access policy become implicit.
- Return null for an unimplemented field: GraphQL nullability and completeness
  regress to runtime failures.
- Stop checking generated code into the repository: generator/runtime drift is
  no longer detectable or reproducible.
- Allow projections on roots: authentication, I/O, and error semantics at the
  operation boundary appear deceptively lightweight.
