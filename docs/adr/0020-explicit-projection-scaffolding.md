# ADR-0020: Preserve explicit projections and reduce only initial authoring with a CLI scaffold

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md)
> and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> define the next target for generated crates, the CLI, configuration, ABI, and
> distribution boundaries (Accepted / Planned). They replace only the relevant
> parts of this decision; existing GraphQL and security contracts and historical
> implementation evidence remain valid.

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-30
- Decision owners: Pythia maintainers
- Depends on: ADR-0002, ADR-0018
- Generated ABI target: 5 (unchanged)

> 2026-10-17 implemented retirement:
> [ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) removes
> projection builders and the scaffold command/library API at ABI 7 cutover,
> without a replacement scaffold or compatibility mode. IDE-assisted complete
> trait implementation is the ordinary workflow. The historical acceptance
> below remains evidence only for its original source.

## 1. Context

The projection plan from ADR-0018 makes a non-root object shorter to implement
than its complete resolver trait. Objects with many fields still require the
initial work of listing `project_*` closures in SDL order. Automatically wiring
domain fields through derive, reflection, or a runtime name map would reduce
typing further, but users would then decide for every object whether automation
applies, whether attributes exclude fields, or whether to return to the complete
builder. That adds ordinary paths and makes field access and cloning or
conversion implicit.

Pythia keeps resolver completeness in the compiler and uses tooling only to
reduce repetitive initial typing, without adding another execution path.

## 2. Decision

### 2.1 Explicit resolution remains the only contract

Each non-root field continues to use one explicit `project_<field>` or
`resolve_<field>` from ADR-0018. Query, Mutation, and Subscription roots use the
complete resolver trait. No derive or procedural macro, domain-field reflection,
default resolver, missing-field null, or runtime field registry is added.

The scaffold emits candidate application source only. It is not a third resolver
form, generated ABI, or runtime contract. Once compiled, scaffolded code has the
same types and execution path as the projection builder written directly.

### 2.2 CLI contract

The following subcommand is added:

```text
cargo pythia scaffold projection \
  --schema-dir graphql \
  --object Book \
  [--config pythia.toml] \
  [--generated-module generated]
```

- `--schema-dir` and `--config` use the same discovery, path, size, and
  diagnostic contracts as `generate` and `check`.
- `--object` accepts exactly one GraphQL object name. `SF4014` rejects anything
  other than a source-defined non-root object.
- `--generated-module` is validated as a Rust path and defaults to `generated`.
  It accepts neither an absolute filesystem path nor arbitrary Rust tokens.
- Success writes exactly one UTF-8 Rust expression to stdout with LF endings
  and a trailing newline. It writes no progress text to stderr.
- The CLI never changes application files, the generated directory, schema,
  configuration, Cargo manifests, or lockfiles. There is no `--write` option.
- Success exits 0. Argument, schema, object, and render errors are tool failures
  that exit 2, matching the existing CLI.

### 2.3 Deterministic scaffold

Output is a complete projection-builder expression in SDL source order.
GraphQL-to-Rust naming shares the generator's model rather than reimplementing
it.

```rust,ignore
generated::projection::Book::<_, _>::builder()
    .project_id(|book| book.id.clone())
    .project_title(|book| book.title.clone())
    .resolve_reviews(|_context, _book, _args| {
        compile_error!("SF4020: implement the Book.reviews resolver");
    })
    .build()
```

- For a field without arguments, output a `project_*` candidate that clones the
  member with the same transformed Rust name. This is an editable candidate,
  not structural typing: the consumer compiler checks accessibility, member
  existence, `Clone`, and the exact output type.
- For a field with arguments, output a `resolve_*` placeholder containing an
  `SF4020` `compile_error!`. Never emit `todo!()`, `unimplemented!()`, a panic,
  null, a default value, or another placeholder that can execute while
  unfinished.
- A user changes the proposed `project_*` to `resolve_*` when a field without
  arguments still needs context, I/O, computation, authorization, or a custom
  conversion.
- Schema descriptions and other arbitrary schema strings are not copied
  unescaped into Rust comments or strings. Any fixed comment is written in
  English in accordance with ADR-0043.
- Identical schema, configuration, object, and module inputs produce
  byte-identical stdout.

### 2.4 Safety and maintenance boundary

The scaffold is neither recorded as generated provenance nor included in
`cargo pythia check`. The source of truth is the SDL and the Rust implementation
committed by the application. After an SDL change, typestate or trait compilation
still reports missing fields. Rerunning the scaffold does not merge application
source automatically.

Output has the same 64 MiB hard limit as code generation. A stdout write failure
does not count as success, even after partial output, and exits 2. Output contains
no secrets, configuration contents, domain source, or absolute filesystem paths.

## 3. Acceptance matrix

| ID | Expected |
| --- | --- |
| SCAF-01 | Fields without arguments are emitted to stdout as `project_*` candidates in SDL order. |
| SCAF-02 | Fields with arguments are emitted as non-executable `resolve_*` placeholders containing `SF4020`. |
| SCAF-03 | Root objects, unknown names, and non-object types are rejected with `SF4014`. |
| SCAF-04 | Naming, keyword escaping, and exact method names match the ordinary generator. |
| SCAF-05 | Identical input produces byte-identical stdout with LF endings and a trailing newline. |
| SCAF-06 | Neither success nor failure changes the repository, generated output, manifests, or lockfiles. |
| SCAF-07 | An edited, compiled scaffold follows the same response and policy path as a handwritten builder. |
| SCAF-08 | The consumer compiler rejects a wrong member, private member, non-`Clone` member, or wrong output type. |
| SCAF-09 | Public API contains no derive, reflection, runtime registry, default/null fallback, or `--write`. |
| SCAF-10 | For a target object in the large consumer, scaffolding finishes within 64 MiB and emits every field exactly once. |

## 4. Consequences

### Positive

- Initial repetitive typing decreases without adding resolver forms or user
  decision branches.
- Because output is ordinary Rust, users review each field's clone, conversion,
  and authorization in place.
- An incorrect scaffold assumption cannot progress beyond consumer compilation.

### Negative

- The final application source is not shorter, and SDL changes still require
  edits.
- A domain member with a different name requires manual correction of the
  candidate.
- Moving stdout into application source remains the responsibility of the user
  or an editor integration.

## 5. Rejected alternatives

- `#[derive(PythiaProjection)]`: adds object-eligibility and escape-route choices
  and makes field access implicit.
- Object mappings in `pythia.toml`: moves mapping away from Rust IDE and type
  checking and expands the configuration surface.
- Generated DTOs or default resolvers: weakens domain type mapping and resolver
  completeness.
- Automatic application-file merging: risks overwriting user-owned code,
  conflicts, and executable unfinished fallbacks.
