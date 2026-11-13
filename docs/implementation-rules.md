# Chigiri implementation rules

Status: Normative

[ADR-0059](adr/0059-craftguild-standards-adoption.md) adopts the immutable
Craftguild v0.1.0 contracts rendered by `make prime`, using the fixed engine in
[standards/engine.json](../standards/engine.json). It supersedes the
physical-line, test-inclusive and 350-line rules in earlier decisions. Existing
historical results retain their original scope. GraphQL, generated ABI, input,
resource, ownership, host, language and review guarantees remain required.

## 1. Implementation size and source roles

Use tokei 15.0.0 **Code**, at most 400 implementation lines per file. Exclude
blank/comment-only lines, tests, test-only helpers, fixtures, ADRs, design
text and vendored code by their actual roles. Explicit source groups live in
`craftguild.toml`. New executable source without a role fails; an unreadable,
unsupported or ambiguous file is unmeasured and cannot pass.

Mixed Rust is parsed before counting. Engine v0.6.1 handles test-only fields,
initializers, variants, match arms, statements and items. It blanks parsed spans
in a temporary source copy,
preserves source lines and potential production feature branches, and uses the
shared separator and counter. Unsupported syntax remains a failure.
It does not edit or compile a different product configuration.

Canonical generated Rust receives a size exception only after complete
ownership, hashes, input provenance and isolated regeneration succeed. The
product registry `scripts/generated-manifests.json`, closed markers and
bounded no-follow inventories remain authoritative. Shared ownership manifests
are normalized from the existing authoritative markers into canonical validation fixtures by
`scripts/refresh-standards-ownership.py`. Every shared assessment then requires
actual isolated rerendered byte equality using the recorded producer identity. Check
never restamps existing generated crates. Fixture roles do not waive ownership.

Authored GraphiQL frontend glue is measured at its source. Compiled dependency
payloads retain the complete asset inventory, embedding/source hashes, locked
licenses, vulnerability audit and isolated esbuild regeneration. Their minified
third-party symbols are not interpreted as first-party test functions.
The Rust embedding table receives no arbitrary size exception.

Do not compress implementation, delete needed tests/comments, hide unrelated
responsibilities in macros or strings, or relabel implementation as a fixture.
Assess responsibility and cohesion even below the threshold. Split by parser,
operation, resource owner or another concrete purpose; avoid numbered shards.

## 2. Checker

```sh
./scripts/check-implementation-rules.sh
```

This retains the product's closed generated Rust/asset ownership, English
language and host-boundary checks, then runs the pinned shared assessment.
`--product-only` retains the product ownership converter without repeating shared
assessment. The converter discovers product groups and checks their exact canonical
records, including generator revisions; shared verification does not replace it.
`--boundaries-only` repeats the closed ownership, language and host checks alone.
During `make check`, the common engine owns canonical reproduction and Code
measurement once. The catalog's first boundary checks product conversion once;
its final boundary uses `--boundaries-only`.
Standalone `full` validation includes one shared assessment at its first boundary.
Standalone `ci` validation is the product catalog; use `make check` for combined
product and common qualification.
The exact tokei counter, explicit source roles, policy findings, measured files,
verified exception candidates and exclusions are recorded in
`.craftguild/implementation-size.json`. Excluded fixtures and measured
implementation are separate scopes even when both have verified ownership.
No failed or missing measurement is recorded as zero.

`MAX_IMPLEMENTATION_FILE_LINES` is retired: setting it returns configuration
error 2. The common contract is fixed. Ordinary failures return nonzero.
The shared gate uses explicitly prepared local Cargo/npm caches and pinned public
sources. Missing prerequisites fail with preparation instructions. It does not install toolchains or
require publisher credentials, a release key, a clean tree or sibling checkout.
Python 3.11 or newer is selected by `python-with-toml.sh` for TOML adapters.

After changing a canonical producer or its inputs, regenerate through its
owning command and rerun the checker. It prepares shared cache automatically.
An explicit diagnostic can also verify all reproduction commands:

```sh
bash scripts/python-with-toml.sh scripts/refresh-standards-ownership.py --verify
./scripts/check-implementation-rules.sh
```

A self-declared generated header, directory, new marker or unknown file cannot
supply an exception. All existing byte/file/depth limits, formatting, compilation,
Clippy, rustdoc, semantic layout and output immutability remain required.

## 3. Integration into implementation work

Run the complete checker before work and its `--product-only` mode after a
subtask and formatting. Run `make check` at integration and before handoff;
it performs the shared assessment without duplicating it in the product catalog.
Resolve its failures before the next subtask. Apply the code-size skill to
changed implementation and assess cohesion as well as the numeric report.
Run directly affected regression/consumer/exact checks while editing, then
`make check` for the fixed-toolchain ordinary product profile. Weekly,
high-risk and formal qualification retain ADR-0058's separate full scopes.
Tests and acceptance use normative contracts and versioned fixtures, never
Beads state or historical review verdicts as their oracle.

Ordinary tests and checks must not create Git commits or signing keys, invoke
signing, or require GPG. Runtime source identities use explicit Cargo metadata-v1
fixtures with synthetic revisions. Real path-Cargo integration remains required;
repository commits and their signing policy are separately authorized work.

## 4. Splitting generated code

The output boundary is the dedicated crate from
[ADR-0026](adr/0026-generated-contract-crate-and-manifest-cli.md), and its file
split follows
[ADR-0028](adr/0028-semantic-generated-layout-and-handwritten-line-budget.md).
Canonical generated Rust has no implementation-size limit; organize files by meaning and
responsibility. The following is an example private layout.

```text
generated/graphql-contract/src/
  lib.rs
  __chigiri/
    types/book.rs
    types/author.rs
    resolvers/query.rs
    resolvers/mutation.rs
    internal/dispatch/book.rs
    internal/...
```

- Re-export public items from `lib.rs`; do not expose the private layout as API.
- Split types, inputs, and enums by owner; Query and Mutation public contracts
  by operation role; and dispatch adapters by object.
- Do not combine everything into one file. Separate public contracts from
  internal dispatch, coercion, descriptors, and other responsibilities.
- Remove sequential packing and method-macro splits used only to meet a line
  count. Retain internal structures needed for type safety, execution, and
  resource control.
- A change to an unrelated model must not alter the paths or bytes of independent
  models and public traits. Distinguish global index or provenance changes from
  real dependency changes.
- Do not warn or reject output solely because a generated file exceeds 400
  lines. Continue enforcing byte, file, depth, and process limits plus formatting
  and compilation quality checks.
- Generate the complete output directory in a temporary location. After checking
  its semantic organization, resource bounds, formatting, compilation, and
  ownership, install it through the existing transaction contract.
- `chigiri rust check` verifies the file list, every file's contents, and the
  absence of extra files at the directory level.

After changing a generator, regenerate with the canonical CLI. Moving
handwritten sources into a generated root to obtain an exemption is prohibited.

## 5. Repository language

Write first-party comments and documentation in English, as required by
[ADR-0043](adr/0043-english-first-repository-language.md).

This applies to:

- module-level documentation (`//!`);
- public and private rustdoc (`///`);
- normal comments (`//`, `/* ... */`);
- explanatory comments in test fixtures;
- explanatory comments in shell scripts; and
- generated-file headers and fixed comments emitted by generators.

Comments should explain:

- why an approach was selected, in addition to what it does;
- GraphQL invariants and the relevant specification sections;
- assumptions about lifetimes, type erasure, concurrency, and cancellation;
- limits and error handling for untrusted input; and
- branches or fallbacks whose purpose is not immediately clear.

Stable identifiers, protocol tokens, media types, error codes, URLs, official
specification titles, and short upstream quotations keep their defined spelling.
Runtime data remains unrestricted Unicode. Generated rustdoc derived from an SDL
description preserves the source data, but repository-owned SDL fixtures and
examples use English descriptions.

English comments are not required on every line. Avoid redundant comments on
obvious assignments and accessors; document public contracts and non-obvious
decisions instead.

The automated repository-language check rejects Japanese text in first-party
files, including source-level Unicode escapes that would render as Japanese.
Its own range definitions and rejection fixtures are the only intrinsic escape
exception. ADR-0043 additionally permits one literal U+5951 name-origin token
on the exact approved etymology line in the root `README.md` introduction,
before its first `## ` heading. The exception does not cover escaped spellings,
duplicates, other contexts/files, or any other Japanese text or punctuation;
the rest of that README remains subject to the normal scan.

Reviewers also verify that:

- new and changed comments are clear English;
- public APIs have the required rustdoc;
- security, concurrency, and GraphQL invariants explain why they exist; and
- comments remain consistent with the code they describe.

## 6. Definition of Done

Track new review rounds, findings, corrections, and independent verification in
Beads, following the [review workflow](review-workflow.md). Implementers record
`Fixed` without closing findings; independent reviewers record `Verified` and
close them. Preserve stable issue IDs and legacy finding IDs. The former
Markdown review ledger was deleted; its records are in Beads epic `lab01-c43`. Do not create
Markdown review sheets or duplicate finding/status tables in validation records;
link supporting evidence from the corresponding Beads issues.

An implementation subtask is incomplete until all of the following hold:

- every implementation file satisfies the shared tokei Code contract, with
  explicit roles, verified canonical Rust exceptions and no unmeasured input;
- the checker passes with its default configuration;
- implementation comments and rustdoc follow this section's English-language rule;
- changed generated code satisfies semantic layout, local-diff, ownership, and
  resource-limit requirements;
- formatting, Clippy, tests, and generated drift checks pass; and
- file splits leave module responsibilities consistent with the architecture and
  ADRs.

[ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md) and the
[validation guide](validation.md) define execution frequency and `ci`/`full`
scope. A small subtask does not require the full product or long-run campaign.
Use focused checks while editing and consolidate the appropriate profile at
handoff. Optimized safety regressions, correctness and resource/ownership
assertions remain mandatory; failed or skipped work never becomes passing
evidence through profile selection.

Canonical shared ownership fixtures live in `tests/fixtures/generated-ownership`.
They contain only data used by validation. Refresh them explicitly with
`bash scripts/python-with-toml.sh scripts/refresh-standards-ownership.py --verify`
after a legitimate generator/input change; `--check` is read-only. The shared
verifier still checks every input/output hash and isolated exact reproduction.
