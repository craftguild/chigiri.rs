# ADR-0028: Partition generated code by semantics and limit the 400-line rule to handwritten code

- Status: Accepted
- Implementation: Implemented
- Validation: macOS local / MSRV and stable / Reviews A and B complete. See
  validation scope and untested environments.
- Date: 2026-09-07
- Decision owners: Pythia maintainers
- Amends: generated line-count and partitioning contracts in ADR-0026/0027 and
  ADR-0002/0018/0024
- Generated ABI target: 6 (no additional bump solely for this change)
- Goal: updated overall goal
- In-flight handoff:
  additional instructions for the active agent

> 2026-10-17 implemented amendment:
> [ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) removes
> projection-specific output units at ABI 7 cutover. Resolver, type, dispatch,
> coercion, and descriptor units continue to follow this semantic layout and
> verified ownership contract. The handwritten limit and generated exemption
> remain unchanged; the layout below records the earlier projection baseline.

2026-11-05 amendment: [ADR-0059](0059-craftguild-standards-adoption.md)
replaces handwritten physical-line/test-inclusive and 350-line rules with
Craftguild v0.1.0 implementation-only tokei Code measurement. The sections
below retain the earlier decision's history; semantic partitioning, ownership,
exactness, immutability, ABI and resource protection remain in force.

## 1. Context and treatment of the decision

During implementation of ADR-0026 and ADR-0027, the decision to apply a
350-line soft limit and 400-line hard limit to generated code was reconsidered.
For handwritten code, a line limit controls the size of a responsibility that a
person implements and reviews. For generated output, the same limit splits one
model or resolver trait across numbered files and disconnects the review unit
and diff from the schema's semantics.

Together with the decision to isolate output in a dedicated generated crate, the
quality criterion changes from a fixed line count to a stable organization by
semantics and responsibility. This ADR amends only the affected parts instead of
replacing the previous ADRs, preserving a clear record of the in-flight decision
change. It does not instruct implementers to revert existing work or redo other
ADR-0026/0027 tasks. The design is accepted, but that status alone does not mean
checker, renderer, and test migration is complete.

## 2. Line-count rules

### 2.1 Handwritten code

Project-owned handwritten `.rs` and `.sh` files remain at most 400 physical
lines, with 350 as the normal split point. This covers generators, renderers,
build scripts, CLIs, runtimes, tests, compile-fail consumers, examples, and
repository scripts. Blank lines and comments count. Minification, removal of
necessary tests or rustdoc, and hiding responsibilities in giant macros or
strings do not evade the rule.

When a test embeds generated Rust in a string literal, that test is still
handwritten code. A `scaffold projection` candidate becomes handwritten once it
is incorporated into an application. Generation history does not permanently
exempt source that humans subsequently maintain.

### 2.2 Canonical generated code

Generated Rust is exempt from both the 350-line soft limit and 400-line hard
limit only when it is produced by the canonical CLI or library, included in the
ownership inventory, and covered by exact checks. Generated crates in goldens,
fixtures, and examples receive the same treatment. Exceeding 400 lines alone
does not warn, reject, or trigger automatic splitting. No replacement arbitrary
line limit is introduced.

Only the line-count rule is waived. Generated output remains immutable by hand
and must pass formatting, Clippy, rustdoc, compilation, and input/output resource
limits. Fixed English headers and rustdoc follow ADR-0043, and the policy for
preserving SDL descriptions remains unchanged.

## 3. Semantic output layout

A schema is not emitted as one giant Rust file. The basic unit is “SDL owner ×
Rust responsibility.” This example describes private implementation layout; its
directory names do not become public API.

```text
src/
  lib.rs
  __pythia/
    mod.rs
    generator_info.rs
    schema_types.rs
    types/
      book.rs
      author.rs
      create_book_input.rs
      book_status.rs
    resolvers/
      query.rs
      mutation.rs
      objects/
        book.rs
    projection/
      book.rs
      author.rs
    internal/
      descriptor/...
      coercion/...
      dispatch/...
```

- Objects, input objects, enums, interfaces, unions, and similar types are
  grouped by the owner that has generated declarations. An object represented
  only by a domain associated type requires neither an empty file nor a new DTO.
- The public resolver contracts for Query and Mutation roots are separated into
  query and mutation responsibilities. Root classification uses the actual SDL
  operation roots, not the literal name `Query`. Ordinary object resolver
  contracts are also separated by owner. No unnecessary file is generated when
  a root is absent.
- Projections are grouped by object. This does not add projections or default
  resolvers to root resolvers.
- Public traits and types may be separate from internal dispatch, coercion, and
  descriptors. There is no requirement to force all processing related to one
  object into one file.
- Assembly, `SchemaTypes`, module and re-export lists, source indexes, and similar
  whole-schema aggregation may use aggregate files. Such files cannot become a
  loophole for unrelated implementations of every model.
- A fixed-width table over a complete index or IR, an acyclic chunk, or another
  structure with a real execution or resource rationale may remain. Packing such
  as `types_00.rs`, method-macro partitioning, and sequential shards created only
  to meet 400 lines are removed. Macros and typestate required for type-safe
  projection stages and complete resolver contracts remain.

### 3.1 Deterministic, local diffs

File paths derive from kind, operation role, schema coordinate, and existing
keyword-safe normalization. Enumeration order, cumulative line count, and the
size of other models never renumber files. Collisions are rejected in advance,
including on case-insensitive filesystems. Existing ABI 6 naming remains; public
Rust names do not change for layout convenience.

Identical input and generator records produce identical files and bytes. Adding
or changing an unrelated model preserves the paths and bytes of existing
independent model-type and public resolver-contract files. Whole-schema
`SchemaTypes`, descriptors, re-exports, source/provenance indexes, and code that
actually depends on the changed type may change. Generator information and the
whole-schema digest remain in aggregate metadata rather than being copied into
every model header. An internal table that requires global numeric IDs is part of
the affected diff, but public model files do not inherit unnecessary global
order dependence.

Generated files live in semantic directories, while private Rust modules are
registered directly below `__pythia` with `#[path]`. Existing `super` references
and public root re-exports therefore remain, and filesystem directories do not
become public API. A private module name derives from the semantic path by
escaping underscores as `_u` and separators as `_p`. Type paths are
`types/<owner>.rs`; resolver paths are
`resolvers/{query,mutation,subscription}.rs` or
`resolvers/objects/<owner>.rs`; public projection façades are
`projection/<owner>.rs`. Internal projection, dispatch, coercion, bridge, and
descriptor code is grouped by owner or aggregate responsibility.
`assembly.rs`, `schema_types.rs`, `modules.rs`, and `sources.rs` aggregate the
whole schema. Existing semantic validation rejects an SDL that uses one type for
multiple operation roots.

Existing public re-exports and private module boundaries remain. Users do not
change imports to a new private path. The generator determines semantic groups;
the manifest and CLI gain no line-count setting, partitioning mode, or old-layout
option.

## 4. Generated ownership and the checker

A `// generated` comment, file or directory name, or glob match is insufficient
for a line-count exemption. The repository links an explicit managed
manifest/output-root list with each output's closed marker and inventory.
Existing manifests are reused where possible; a duplicate source of truth is not
created.

- An exempt generated `.rs` file must be a regular file listed in the validated
  inventory of a recognized output root.
- A same-named directory outside a recognized root, self-declared header,
  arbitrary marker, or newly introduced unregistered file is not exempt.
- A missing or invalid marker, unknown file, symlink or path escape, or traversal
  failure in a recognized generated root cannot pass. Handwritten content placed
  there is an ownership violation even when short.
- The line checker does not require Cargo builds, generation, or authentication.
  It performs bounded ownership validation and handwritten line checks. The same
  mandatory aggregate's exact check proves regenerated byte equality. The marker
  is not an authenticity signature.
- Reports distinguish the number of handwritten files checked from generated
  files excluded; they do not make generated files disappear and report only a
  successful count.
- Temporary checker fixtures also construct explicit ownership. No production
  option skips checks or relaxes the limit.

The repository's managed manifest list is
`scripts/generated-manifests.json`, shared by the checker and mandatory exact
checks. Output paths are not duplicated there; each is resolved from its
manifest's `generation.output`. The checker requires no external package or
Cargo and uses standard `tomllib` from Python 3.11 or later.
`python-with-toml.sh` selects an installed compatible interpreter and exits
nonzero if none exists. The marker is closed at every level, records are sorted
and unique, byte hashes match the actual tree, and empty directories and case
collisions are rejected. Product CLI exact checks remain responsible for
manifest/input drift and rerendered agreement between Rust and JSON producer
records. The ownership marker is not a signature.

Implementing this generated exemption as designed is within scope and does not
violate the previous general prohibition on exemptions. Excluding unrelated
handwritten source remains prohibited. If checker or renderer migration is
incomplete, record it as such instead of adding more mechanical shards merely to
satisfy old generated-line assertions.

## 5. Preserve safety, ABI, and validation

File-count, byte-count, and depth limits; bounded rendering and reading;
formatter batching and process deadlines; marker/provenance checks; directory
transactions, rollback, and recovery; and read-only exact checks all remain.
Permitting more than 400 lines does not allow unbounded concatenation or repeated
cloning of complete buffers. Reaching an existing byte or process limit is a safe
rejection diagnosed separately from line-count policy.

This ADR changes file placement and line-count policy, not GraphQL semantics,
SDL `SchemaDigest`, binding fingerprints, generated public API, complete
resolvers or projections, or runtime ABI target 6. A layout-only change is not a
GraphQL breaking change. Generated bytes and inventory necessarily change, so
canonical output is regenerated with update after renderer changes. No
old-layout mode or hand-edited generated file is retained.

Existing PROJ-11, large-consumer, and quality traces replace generated 400-line
assertions with semantic-layout and resource-limit assertions. Handwritten
400-line, output-byte, compilation, and other assertions remain. Historical
Passed records remain historical and are not rewritten to claim that the new
policy was already validated.

## 6. Implementation and completion conditions

Continue the in-flight ADR-0026/0027 work while adapting output writers and
renderers, the repository checker, testkit, goldens, examples, the large
consumer, CI, and validation traces to this ADR. The implementation agent makes
the source changes.

SG-LAYOUT-01 through SG-LAYOUT-08 in
[spec-conformance Section 16](../spec-conformance.md#16-semantic-generated-layout-conformance)
form the additional Gate SG-F. A representative generated model or root trait
over 400 lines succeeds, while a 401-line handwritten renderer or consumer
fails. Local diffs, collisions, drift, and resource limits receive independent
checks. Existing SG-A through SG-E remain, and the complete suite runs once at
the end. Independent Reviews A and B also inspect layout and exemption scope.

This change does not edit external applications or restore publish, SSH, or commit
waiting to completion gates. A layout-only change does not require another
configuration, marker, or ABI version bump. Safe update from the existing marker
and drift rejection under the new renderer remain. Any newly required wire or
public contract change needs a separate rationale.

## 7. Consequences

Generated output becomes easier to navigate from schema semantics, and unrelated
files move less often. Long Query or Mutation files are acceptable; responsibility
separation and local diffs matter more than the superficial 400-line shape. The
first regeneration creates a broad layout diff but requires neither a consumer
public-API change nor developer hand edits. Handwritten review budgets coexist
with separate checks for generated reproducibility and resource safety.
