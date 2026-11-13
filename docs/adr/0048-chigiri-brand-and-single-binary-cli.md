# ADR-0048: Chigiri brand and single-binary CLI

Status: Accepted

Date: 2026-09-23

Implementation: Implemented

The implementation goal prompt
preserves the self-contained execution, validation, and handoff instructions.
Final-source evidence is recorded in the
validation record.

2026-10-17 implemented amendment:
[ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) removes
`chigiri rust scaffold` at the trait-only ABI 7 cutover, without a replacement
or compatibility alias. The single binary, branding, remaining commands, and
this ADR's implementation history are preserved. The command examples below
record the historical ABI 6 workflow, not the current command inventory.

## 1. Context

The product supports two complementary schema-first workflows: generated Rust
contracts for application implementations, and disposable, stateful Mock
servers that let frontend work proceed before those implementations are ready.
Both start with GraphQL SDL, but currently have separate entry points:
`cargo-pythia` (invoked as `cargo pythia`) and `pythia-mock`. The generator is
not a binary named `pythia-cli`.

ADR-0047 improved Mock authoring boundaries. The next product step is a clear
identity and a coherent installation and first-use experience, not another
expansion of the resolver language. Adopt Chigiri, a name referring to a pledge
or covenant, to emphasize the schema contract shared by the two workflows.

The Rust command surface also carries schema snapshot/diff functionality that
is not required to generate, update, or verify Rust contracts. Its production
callers are the dedicated CLI commands; Mock project updates use independent
comparison machinery. Retaining those commands solely for possible future
compatibility tooling would enlarge the v1 maintenance surface.

This ADR specifies a clean break, not a compatibility layer. Acceptance of the
design does not mean the rename, binary integration, distribution, or production
verification has already happened.

## 2. Superseded and preserved contracts

At implementation cutover, this ADR takes precedence only in these areas:

| Earlier decision | Revised scope |
| --- | --- |
| [ADR-0013](0013-schema-change-and-release-governance.md), Sections 2.1 and 2.2 | Withdraw the product-owned schema-wide snapshot/diff facility and its dedicated release gates. Preserve independent operation-catalog validation, testkit, and runtime integrity contracts. |
| [ADR-0020](0020-explicit-projection-scaffolding.md) and [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md) | Replace CLI spelling, branded configuration/artifact identities, and the public Rust command inventory. Preserve projection behavior and generated-contract ownership semantics. |
| [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md) | Replace the generator binary acquisition/build identity. Preserve lightweight validation and the separation of product completion from publisher/release evidence. |
| [ADR-0029](0029-experimental-luau-mock-cli.md) and [ADR-0035](0035-cli-tui-and-embedded-graphiql.md) | Replace separate public binaries with one composed CLI and extend the higher-MSRV boundary to that adapter. Preserve Mock isolation, parser behavior not explicitly changed here, and embedded runtime assets. |
| [ADR-0032](0032-mock-project-layout-and-portable-export.md) and ADR-0044 through ADR-0047 | Rename active Mock format families, generated vocabulary, and guidance. Preserve project ownership, sandbox, storage/output boundaries, time, diagnostics, and execution semantics. |

Historical ADRs and validation records retain their factual names, commands,
results, and limitations. Active implementation guides and gates must be
updated during cutover rather than treating historical examples as aliases.
Until then, the existing binaries remain the implemented interface.

## 3. Public command surface

Ship one public product binary, `chigiri`, with two explicit namespaces:

```text
chigiri rust init
chigiri rust codegen
chigiri rust update
chigiri rust check
chigiri rust doctor
chigiri rust scaffold --object Book

chigiri mock init
chigiri mock update
chigiri mock check
chigiri mock serve
chigiri mock export
chigiri mock reference
```

The examples omit existing command-specific options. Bare `chigiri`,
`chigiri rust`, and `chigiri mock` display help without selecting an action.
Do not expose a Cargo plugin, a `gen` or `generate` alias, a `projection`
subcommand, or compatibility entry points named `cargo-pythia` or
`pythia-mock`. Removed commands must fail as usage errors, not silently route
to another operation. The retired Mock `infer`/`apply` workflow stays retired.

### 3.1 Rust responsibilities

| Command | Responsibility |
| --- | --- |
| `init` | Establish the manifest-driven Rust project configuration using the existing initialization ownership rules. |
| `codegen` | Replace the name `generate`; create the initial dedicated generated crate at an absent output destination. Do not adopt or overwrite an existing directory. |
| `update` | Regenerate an existing, verified owned output tree, retaining transactional publication and recovery behavior. |
| `check` | Compare current inputs with the exact owned generated output, including inventory, bytes, provenance, and applicable ABI/fingerprint checks. Do not run consumer Cargo validation. |
| `doctor` | Include `check`, then verify the configured real consumer's resolved dependencies and Cargo build under the existing locked-validation contract. It is not a synonym for `check`. |
| `scaffold` | Print an editable Rust projection expression for one source-defined non-root object to stdout. |

`scaffold` is only a shorter spelling of `scaffold projection`. Keep projection
terminology in help so its narrow purpose is clear. Preserve deterministic
output, object selection, domain-field defaults, explicit unresolved resolver
placeholders, and existing rejection rules. It does not create files, modify
application code, replace `init`, add `--write`, or introduce scaffold kinds.

`codegen` and `update` retain validation of the generated crate; they do not
gain `doctor`'s real-consumer responsibilities. Exact generated-output checking
does not claim client/API compatibility between two schema revisions.

### 3.2 Mock responsibilities and common CLI behavior

Move the current `init`, `update`, `check`, `serve`, `export`, and `reference`
operations under `chigiri mock` without changing their responsibilities.
Full Mock `check` remains the readiness authority; a scoped check does not
become one. Preserve external-SDL references, user-owned edits, portable
export, restart behavior, and update/recovery rules.

Except for the names and removals specified here, preserve namespace-specific
options, defaults, stdout/stderr separation, diagnostics, and exit semantics.
Do not impose a new root-level output format: Rust's `human|json` and Mock's
`text|json` options remain where supported by their respective commands.

Help and version paths must work offline without loading project files,
starting a VM, invoking Cargo or Git, or writing files. Root version identifies
the Chigiri build; Mock version continues to report the embedded Luau version.
Preserve actionable configuration and usage failures without starting unrelated
namespace machinery. `doctor` remains a trusted Cargo workflow, not a sandbox
or an implicit promise of offline dependency resolution.

Root-help presentation clarification (2026-09-27): bare `chigiri` and root
help (`--help`, `-h`, or `help`) prepend the existing ASCII product banner
once, followed by a blank line. Namespace and command help remain banner-free.
This applies equally to terminal and redirected output. Existing version
output, namespace-specific writers, and exit semantics remain unchanged.

## 4. Remove Rust schema snapshot/diff, not integrity checking

Remove the Rust `snapshot` and `diff` commands completely. Remove their
exclusive library APIs and types, canonical governance snapshot serialization,
snapshot readers/writers, semantic change classification, report formats,
dedicated fixtures, tests, documentation, and validation gates. In particular,
do not create Chigiri replacements for `pythia/schema-snapshot-v2` and
`pythia/schema-diff-v2`. Hiding commands while retaining an unsupported public
library feature does not satisfy this decision.

Compatibility review could justify a new proposal later, with an actual
consumer and a bounded contract. It is not a reason to keep this implementation
in the active product now; repository history remains available.

The removal expressly excludes:

- Mock's internal `generated/schema.snapshot.json`, update-state schema
  comparison, change summaries, and project update/recovery machinery.
- Rust `SchemaDigest`, generated ABI, binding/resolver fingerprints, ownership
  markers, verified inventories, exact generated-output checks, and the
  schema representation required by code generation and execution.
- Mock stored-value and query/store snapshot semantics, transaction rollback,
  and restart behavior.
- Independent operation-catalog validation, runtime integrity checks, testkit
  response comparison, and unrelated conformance assertions.

Audit callers and validation gates by responsibility, not by the word
`snapshot` or `diff`. Retire dedicated governance checks such as SG-CRATE-07.
Move still-required assertions in shared tests such as SG-LAYOUT-07 to the
retained descriptor/layout APIs before removing their governance helper. Do not
delete unrelated coverage just because it currently imports a removed API.

## 5. Binary composition and dependency boundaries

Introduce the `chigiri_cli` adapter package at `crates/cli`, owning the sole
public product binary `chigiri` and one clap command tree. Rust and Mock command
adapters directly call their respective libraries. Remove the old binary
targets and Cargo-plugin argument normalization; do not spawn either former
CLI or retain hidden compatibility wrappers.

Extract command plumbing along named responsibilities. The codegen and Mock
libraries must not depend on the new CLI, and normal runtime/framework crates
must not acquire CLI or Luau dependencies. Library functions do not terminate
the host process; exit handling belongs at the binary boundary. Preserve
namespace-specific signal handling, cancellation, diagnostics, and shutdown.

The Mock stdio writer remains a bounded helper process launched through the
current executable. Rename its internal command to
`__chigiri-stdio-writer-v1`, route it through the same binary, and preserve its
strict hidden grammar, pipe limits, and teardown behavior. It is not a public
authoring command. One binary does not mean one process, and relocation must
not make this helper depend on a repository path or separately installed tool.

### 5.1 Toolchain and host boundaries

- Normal libraries, the codegen library, and generated Rust consumers retain
  edition 2024 and MSRV 1.85. The core remains standard-library-only.
- Mock and the unified CLI require Rust 1.95. Keep both outside the default
  MSRV-1.85 workspace member set and validate them explicitly. Move CLI-only
  tests to the appropriate higher-MSRV validation lane without weakening
  library or generated-consumer checks.
- Rust workflows still need Cargo and the existing rustup-managed, fixed
  rustfmt 1.85.0 toolchain where required. Do not install toolchains implicitly.
- Mock runtime execution embeds its Luau and existing assets; it does not
  require an external Rust compiler or Luau executable.
- Preserve the CLI host scope of Linux and macOS. This ADR does not add Windows
  host support or change independent runtime/consumer target portability.

The single binary is the product entry point, not a promise that Rust builds
need no external tooling, configuration, SDL, dependencies, or network access.

## 6. Product-wide identity change

Rename active product-owned surfaces consistently, not only the executable:

| Current identity | Chigiri identity |
| --- | --- |
| Product Pythia; Rust facade `pythia` | Product Chigiri; Rust facade `chigiri` |
| Workspace packages/imports `pythia_*` | Corresponding `chigiri_*` packages/imports |
| Generated Luau `Pythia*` types | Corresponding `Chigiri*` types |
| Product-owned generated/private `__pythia*` names | Corresponding `__chigiri*` names |
| `pythia.toml`; `pythia-generated.json` | `chigiri.toml`; `chigiri-generated.json` |
| Branded private transaction, recovery, and cache names | Corresponding Chigiri-owned names |
| Active product-owned environment variables, diagnostic origins, help, and build identity | Chigiri spellings with the existing behavior |

Apply the rename to producers, consumers, fixtures, manifests, dependency
aliases, scripts, active guides, and validation inventories together. Neutral
Mock names such as `mock.luau`, `storage.graphql`, `generated/types.d.luau`,
and `generated/schema.snapshot.json` remain unchanged.

Do not blindly replace user SDL names, application/domain identifiers or data,
third-party code, external URLs, or historical evidence. An old-name rejection
fixture is intentional. Audit and explain retained occurrences rather than
demanding zero occurrences of the word Pythia across the repository.

### 6.1 Formats, versions, and fingerprints

Rename retained Rust format prefixes from `pythia/` to `chigiri/`, including
configuration, generated-crate marker, generator information, diagnostics, and
recovery identities. Rename each retained Mock family from `pythia-mock*` to
`chigiri-mock*`:

| Mock family | Retained version |
| --- | --- |
| Project / portable bundle | v6 / v8 |
| Host API / capabilities | v7 / v2 |
| Full reference / topic reference | v9 / v5 |
| Check / runtime log | v2 / v3 |
| Update state / internal schema snapshot | v7 / v7 |
| Directive vocabulary | v3 |

The prefix change establishes a new identity even when the numeric version
stays the same. Do not advance numeric versions, generated Rust ABI 6, or the
workspace's current 0.1.0 package version solely to rename the product. If
implementation needs an additional structural or semantic change, document
and version it explicitly rather than bundling it into the rename.

Rename product-owned fingerprint domain separators and synthesized source
labels, including `pythia/domain-enums-v1` and `pythia-mock/v3.graphql`.
Recompute affected fingerprints and provenance from the renamed producers;
brand-bearing hash inputs do not retain their former hash bytes. Preserve
the `SchemaDigest` algorithm for identical relative SDL paths and source bytes.
A renamed synthesized Mock vocabulary source can change its composed digest.
Unbranded diagnostic codes and numerical limits keep their existing contracts.

### 6.2 Clean-break migration and ownership

Do not read old-branded formats as Chigiri formats, search for `pythia.toml` as
a fallback, or adopt an old generated marker as new ownership. Reject old or
mixed configuration/artifacts before mutation of the affected project or
output. No automatic converter, compatibility reader, or cross-brand updater
is included.

Migration creates a fresh project/output destination with the new `init`
workflow and explicitly adapts user-owned sources and application imports.
Preserve original projects, recovery artifacts, and user edits; do not migrate
by overwriting them. For repository fixtures and examples, regenerate canonical
outputs through the renamed producers and verify ADR-0028 ownership/inventory
rules. Do not hand-edit generated files, hashes, or inventories into agreement.

## 7. Implementation packages and acceptance

Implement in bounded packages, running the implementation-rule checker after
each and retaining reviewable ownership boundaries:

1. Remove dedicated Rust governance APIs/commands/gates and relocate shared
   assertions to retained APIs; prove Mock update comparison is unaffected.
2. Introduce the single CLI adapter and namespace grammar, including the
   renamed scaffold command, same-executable writer, and toolchain separation.
3. Complete the product-wide rename, regenerate owned artifacts, and update
   active guides, scripts, fixtures, and validation entry points.
4. Run cross-workflow acceptance and the updated full product validator; record
   exact source identity, executed environments, and unexecuted release gates.

Intermediate changes may be organized by responsibility, but the transition is
not complete with mixed brands, stale ownership, or only one working namespace.
All acceptance items below are required and Passed locally on macOS. The
validation record maps each item to focused evidence, the relocated executable
run, the complete product validator, and the two independent reviews. Linux,
Windows, remote CI, publishing, packaging, and signing remain unexecuted.

| ID | Required evidence |
| --- | --- |
| CH-01 | Root and namespace help/version work offline without project/tool execution or writes. Invalid and retired commands/options produce the intended usage failures; no compatibility aliases remain. |
| CH-02 | One built binary, copied outside its build/repository directory, executes both workflows with normal project inputs and required Rust tooling, without either old CLI on PATH or repository-relative launchers. |
| CH-03 | Real Rust `init`, `codegen`, `check`, `update`, and `doctor` succeed against a standalone generated crate and real consumer. Preserve MSRV-1.85 consumer coverage and ownership, drift, dependency, and recovery negative cases. |
| CH-04 | `rust scaffold --object Book` emits deterministic projection source without file changes. Preserve invalid-object rejection and explicit unresolved placeholders; the adapted valid projection compiles in a consumer. |
| CH-05 | Mock init/update/check/reference/serve/restart/export and recovery pass with the renamed project/bundle formats. HTTP acceptance retains nested output, authentication, rollback, private-data secrecy, and diagnostics coverage. |
| CH-06 | The relocated executable starts its own hidden stdio writer. Malformed hidden invocations, bounded pipe behavior, cancellation, and shutdown retain their safety tests. |
| CH-07 | Removal inventory shows no active Rust governance command, public API, serializer, classifier, or dedicated gate. Shared descriptor/layout assertions and Mock internal snapshot/update tests still pass. |
| CH-08 | Brand inventory covers active public/private identities, generated names, environment variables, docs, fixtures, and formats, with an explicit allowlist for history, external/user data, and rejection fixtures. |
| CH-09 | Old/mixed inputs are rejected before mutation. Fresh-destination migration and regeneration preserve user-owned originals and pass exact output/inventory verification. |
| CH-10 | Dependency and toolchain checks demonstrate no CLI/Luau leakage into normal libraries or generated consumers, MSRV separation, and preservation of the std-only core. |
| CH-11 | Formatting, Clippy, rustdoc, compilation, implementation/language rules, retained tests, and the updated full product validator pass on the final source. Record Linux/macOS evidence separately; unexecuted environments remain unexecuted. |

The documentation-only adoption of this ADR does not satisfy those acceptance
items. Its own checks are document consistency, local links, implementation
rules, and diff hygiene. Existing Pythia validation records are not renamed
and presented as evidence that Chigiri passed.

## 8. Consequences and non-goals

Users learn one executable with two explicit workflows. The command inventory
is smaller, while generated Rust correctness and Mock update safety remain
independent of the removed schema-governance feature. The combined CLI has a
higher build-toolchain requirement and includes Mock dependencies even for
Rust-only CLI users; normal Rust consumers do not inherit that cost.

The clean break requires explicit application/configuration migration. Keeping
old aliases, shipping two public binaries, or hiding rather than removing the
governance library would reduce immediate migration work but retain the
maintenance and discovery surfaces this decision removes.

This ADR does not add installers, release archives, registry publication,
signing, release CI, domain/trademark acquisition, or a package-version release
policy. It does not add external network capabilities to Luau, nullable-flow
refinement, new scaffold kinds, or Mock authoring features. Naming and binary
integration do not certify production readiness or substitute for the
separate product and release evidence required by the existing architecture.
