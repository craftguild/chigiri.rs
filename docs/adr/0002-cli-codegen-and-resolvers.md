# ADR-0002: Use checked-in CLI code generation and complete resolver traits

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md) and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md) define the next generated-crate, CLI, configuration, ABI, and distribution boundaries (Accepted / Planned). They replace only the affected areas; existing GraphQL and safety contracts and historical implementation evidence remain.

> 2026-09-07 amendment: [ADR-0028](0028-semantic-generated-layout-and-handwritten-line-budget.md) replaces generated 350/400-line limits and line-count sharding with semantic decomposition.

Status: Accepted
Date: 2026-08-26

## Context

Pythia must detect drift between SDL and Rust implementations and turn schema changes into application compiler errors. The generation choices are `build.rs`, procedural macros, or an explicit CLI. The design must also choose between Apollo-style partial field registration and mandatory implementation of every field.

## Decision

### Generation workflow

- `cargo pythia generate --schema-dir ... --output-dir ...` is the canonical generator.
- Generated Rust source is checked into the repository.
- CI uses `cargo pythia check` to require byte-identical output.
- Invalid SDL or generation errors leave existing output unchanged.
- The generator uses pinned rustfmt 1.85.0 with embedded configuration. Historically it treated 350 formatted lines as a soft shard boundary and 400 physical lines as the maximum. An indivisible item over 350 lines could occupy a dedicated shard if it remained within 400 including required headers; larger items produced diagnostics rather than minification. ADR-0028 now governs semantic generated layout.
- The historical layout split source lists into `sources_*.rs` and module/re-export lists into fixed-width `modules_*.rs` and `module_index_*.rs`, keeping `mod.rs` independent of schema width. Large descriptor arrays used fixed-width acyclic chunk tables that safe constructors could not turn into cycles. ADR-0028 supersedes purely line-driven layout choices.
- Public items are re-exported from `mod.rs`; shard filenames are not public API.
- Generation occurs in a temporary directory. The output directory is replaced only after file inventory, contents, and applicable quality checks pass. This guarantees a process-level transaction for success and recoverable I/O errors, not crash-atomic visibility between renames.
- `./scripts/check-implementation-rules.sh` independently validates generated sources.
- Version 1 provides neither `build.rs` generation nor procedural macros.

### Resolver completeness

- Generate one resolver trait per object type, including operation roots.
- If a root object is also a nested output, retain one trait/group and pass `Option<&T::RootType>` to each method: `None` at the operation root and `Some` for nested resolution.
- Every SDL field is a required trait method.
- Provide no property reflection or default resolver.
- Generated `Resolvers::new` requires every resolver group.
- Adding either a field or an object becomes a compile error until implemented.

### Async API

Public resolver traits use return-position `impl Future<Output = ...> + Send`, allowing application implementations to use `async fn`. Only generated private adapters erase futures and resolver groups behind object-safe interfaces. Output objects erase to owned `Any + Send + Sync + 'static`; list-item `FieldResult` values remain distinct until value completion.

### Schema artifact

Generated source embeds a validated static schema descriptor, so startup does not reparse SDL. Mismatched generated and framework ABI versions are compile errors.

## Considered options

### `build.rs` and `OUT_DIR`

This prevents forgotten generation but degrades rust-analyzer behavior, diagnostic visibility, reviewable diffs, and cross-crate include paths.

### Procedural macro

Declarations are concise, but file discovery, multi-source diagnostics, incremental compilation, and generator versioning become difficult.

### Runtime resolver registry

This enables flexible module registration, but missing fields become startup errors and compile-time completeness is lost.

### Generated output DTOs

They permit default resolution of simple fields but introduce eager loading and duplicate domain representations.

## Consequences

- SDL/generated API changes are reviewable.
- Developers must run the generator after schema changes.
- Simple fields still require resolver methods, so the implementation is more verbose than an Apollo resolver map.
- Generated source is visible but never edited by hand.
- Private shard counts may change without changing application import paths.
- `Resolvers::new` can have many arguments for large schemas. Version 1 favors completeness over a typestate builder.

## Revisit triggers

- Generated-directory merge conflicts or shard churn remain a recurring problem.
- Large schemas make `Resolvers::new` compiler performance or ergonomics unacceptable.
- Rust tooling provides sufficient diagnostic and IDE support for procedural macros that read source files.
