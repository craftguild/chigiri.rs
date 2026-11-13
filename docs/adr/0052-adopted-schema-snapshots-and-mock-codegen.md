# ADR-0052: Adopted schema snapshots and explicit Mock code generation

Status: Accepted

Date: 2026-09-27

Implementation: Implemented. AS-01 through AS-10 passed locally, and both
independent reviews closed after focused correction re-review. Evidence is recorded in the
ADR-0052 validation record.

Implementation goal: Prompt for a new agent.

## 1. Context and decision

Mock authors need the GraphQL descriptions that explain their types, fields,
arguments, and models. Scoped `reference` should provide these descriptions
alongside the exact contract used to generate and check their implementation.
Reading a newer external SDL separately can mix two versions of that contract.

Today, project loading rebuilds the IR from the current public and private SDL.
`generated/schema.snapshot.json` holds summaries and generation metadata, not
enough SDL to rebuild the adopted public schema. Project reference reads live
SDL without requiring this metadata; validation instead rejects schema drift.
Adding descriptions to reference alone would not resolve that authority split.

Adopt a public schema explicitly, retain its normalized SDL in the existing
snapshot JSON, and use that adopted contract for project authoring and runtime.
Separate public-contract adoption from local generation:

- `init` adopts the initial public SDL and creates a new project.
- `update` adopts the configured public SDL and regenerates in one operation.
- `update --check` compares public SDL without adopting it.
- `codegen` regenerates from the adopted public SDL and current local inputs.
- `check` validates internal contracts, not external schema freshness.
- Project `reference` explains an internally consistent adopted contract.

The external SDL remains the upstream authoring source. Its adopted snapshot
is the sole public-schema authority within a Mock project. There is no second
editable public SDL in the project and no automatic refresh or fallback.

## 2. Supersession and preserved boundaries

At implementation cutover, this ADR supersedes only these earlier decisions:

| Earlier contract | Replacement |
| --- | --- |
| [ADR-0032](0032-mock-project-layout-and-portable-export.md), Sections 3, 4.1, and 8 | Replace summary-only snapshot authority, per-command external SDL loading, and management-independent project reference with the adopted-schema boundary. |
| ADR-0032, Sections 5 and 7 | Split public adoption and local generation into `update` and `codegen`; share the existing placement and recovery rules. |
| [ADR-0044](0044-composable-agent-ready-mock-authoring.md) | Generated guidance directs project authors to adopted-contract reference; project reference requires generation consistency, but still not resolver or seed readiness. |
| [ADR-0045](0045-stateful-mock-authoring-and-runtime-contract-parity.md), Section 6.3 | Private SDL is confined to the project; edit storage, then run `codegen`, without adopting public SDL. |
| [ADR-0051](0051-mock-contract-validation-without-completion-state.md), Section 5 | Apply the narrow version changes in Section 8 below. Preserve completion-free validation and intentional-error scaffolds. |

Keep public/private visibility, typed model operations, readonly request state,
phase permissions, provenance, clocks, request rollback, failure precedence,
resource limits, generated ownership, and safe diagnostics. Normal Rust
codegen, ABI 6, MSRV, dependencies, and product platform tiers are unchanged.
Historical acceptance remains evidence for its original source and contract.

## 3. Snapshot, normalization, and identities

### 3.1 Extend the existing snapshot

Extend `generated/schema.snapshot.json`; do not add a second SDL snapshot file.
Retain its generation, summary, configuration stamp, and combined-schema
identity. Add a closed `public_schema` object containing:

- `normalization`: `chigiri-mock-sdl-normalization/v1`;
- `sources`: the ordered array of `{name, sdl}` normalized public sources;
- `digest`: the lowercase SHA-256 identity defined below.

Names are the existing logical `schema/001.graphql` sequence, not physical
paths. The payload contains the configured public sources, not private SDL or
the automatically appended vocabulary source. Vocabulary composition remains
the existing IR responsibility and remains bound into the combined identity.

Private SDL stays in user-owned local files. Do not keep a second adopted copy
of storage or configuration merely to allow reference of an older local draft.
The existing summary is a derived index of the combined adopted-public and
current-private contract, never an independent source of meaning.

The snapshot and other generated artifacts are not hand-editable. Readers
verify supported formats, canonical payloads, hashes, and shared generation;
internal contract readers also rederive the summary and generated declarations.
Never accept stored type declarations or summaries as an alternative schema.
Missing, corrupt, mixed, or interrupted generations fail closed, including
project reference. Neither `codegen` nor `update` repairs them by silently
adopting external SDL. Retain the existing explicit recovery procedure.

### 3.2 One bounded normalization rule

Use one shared Mock SDL normalizer built on the existing GraphQL lexer/parser,
not regular-expression comment removal or a new parser. Apply it to public
adoption, public comparison, and private-schema generation consistency.

Normalization v1 has these rules:

1. Validate the original input before rewriting, so token serialization cannot
   repair invalid syntax. Preserve source boundaries and configured order.
2. Drop ignored whitespace, line terminators, insignificant commas, BOMs outside
   strings, and `#` comments. Retain all non-ignored tokens in their order.
3. Emit names, punctuators, and numeric lexemes unchanged. Emit every string,
   including block strings, as a quoted GraphQL string containing its decoded
   value. Escape quotes, backslashes, and control characters deterministically;
   use short escapes for backspace, form feed, newline, carriage return, and tab,
   and lowercase four-digit Unicode escapes for other U+0000 through U+001F.
4. Separate tokens with one ASCII space and terminate nonempty output with LF.
   An empty/comment-only source normalizes to an empty string; whether it is
   admissible still follows the existing public/private schema rules.

Do not sort definitions, fields, arguments, directive applications, enum
values, or input-object entries; merge source files; remove unused types;
normalize numbers; or perform Unicode normalization. Description content is
preserved as the GraphQL string value, including meaningful whitespace.
`#` inside a string is data, not a comment. Equivalent quoted/block string
spellings normalize equally when their decoded values are equal.

The rule must be deterministic and idempotent, and reparsing must preserve the
ordered schema meaning and descriptions. A field reorder or file repartition
can require update even when GraphQL validation accepts both versions. This
is a normalized-source comparison, not a semantic-equivalence engine.

Enforce the existing aggregate raw-input SDL bounds before normalization and
the same SDL bounds on normalized output. Charge escaped JSON, metadata,
staging, reference, and bundle output to their existing aggregate budgets.
Expansion is a bounded error, not permission to increase limits.

### 3.3 Separate freshness from internal integrity

The public digest hashes the UTF-8 compact JSON encoding of
`[normalization, [[name, sdl], ...]]`, using the existing deterministic JSON
string encoder and no trailing newline. Array order and logical names are
significant. Physical paths, private SDL, configuration, generation IDs, and
timestamps are not inputs. Changing a public source path to equal contents
does not by itself change the adopted public contract.

The internal generation identity separately binds the adopted public digest,
combined public/private schema identity, current generation-bound configuration,
derived declarations, summary, and existing ownership inventory. Reuse the
current configuration stamp: hooks, state declarations and initial values,
and clock settings still require regeneration when changed; server listen and
CORS settings retain their current runtime-only treatment.

Do not compare the hash of the entire snapshot JSON to the external SDL hash.
`codegen` may change internal metadata but must preserve the adopted public
payload and public digest. Description changes affect the public digest even
if generated Luau declarations are unchanged. Summary change reporting must
not label such an adoption as having no public-schema change.

These hashes detect consistency and accidental edits, not authenticity against
an actor able to rewrite all files and hashes.

## 4. Inputs and command boundaries

`schema.sources` retains explicit ordered paths relative to `mock.luau` or
absolute paths. It is an acquisition locator, not live project schema data.
Changing a locator does not trigger adoption. Only `init`, `update`,
`update --check`, and explicitly source-based `reference --schema` open public
SDL paths. Internal commands validate the configuration shape but do not stat,
open, hash, or require existence of those paths, even if located inside the
project. No implicit filesystem discovery is introduced.

`storage.sources` must contain explicit portable project-relative file paths.
Reject absolute paths, traversal, symlink components, and escapes from the
retained project directory using the existing host boundary. Retain duplicate
file, alias, bounded-read, and concurrent-change checks. An empty list or an
empty/comment-only private source remains valid. `init` still creates the
editable commented `storage.graphql` sample; no copy or import of external
storage is implicit.

| Project operation | Public schema used | Local schema/configuration | Writes |
| --- | --- | --- | --- |
| `init` | Explicit source inputs, normalized and adopted | Generated initial local inputs | New project only |
| `update` | Current configured source inputs, normalized and adopted | Current local inputs | One protected generation |
| `update --check` | Compare current configured sources with adopted public payload | Read settings and verify existing generated ownership, not pending local schema consistency | None |
| `codegen` | Adopted payload only | Current local inputs | One protected generation |
| Full/target `check` | Adopted payload only | Require current generation consistency | None |
| `reference --config-dir` | Adopted payload only | Require current generation consistency, not module/seed validity | None |
| `serve` / `export` | Adopted payload only | Full current-input validation | Existing server/export behavior only |

Public source modification, invalid contents, relocation, or deletion cannot
break internally valid project reference, validation, serving, or export.
When local schema or generation-bound settings differ, internal readers stop
with a bounded diagnostic directing the author to `chigiri mock codegen`.
Malformed local inputs receive their actual diagnostic, not a claim that
running codegen will fix invalid SDL. Generated corruption is an ownership
failure, not ordinary pending local work.

Evaluate `mock.luau` only once per operation. Reuse captured settings and
retained file snapshots through validation, comparison, and publication.
Project reference may verify generated files, but must not read/type-check
resolver bodies, inspect seeds, construct the store, or report readiness.
Full and target checks keep their existing scope semantics and never invoke
entrypoint bodies to infer business completeness.

## 5. CLI and generation behavior

Add these operations to the existing closed Mock command tree:

```text
chigiri mock codegen --config-dir DIR
chigiri mock update --config-dir DIR --check [--format text|json]
```

`--format` defaults to text and is accepted on `update` only with `--check`.
Ordinary `update` and `codegen` share placement reporting; neither success
claims readiness. No `generate` alias, local-update flag, new schema-diff
command, implicit codegen-on-check, or force-overwrite option is added.

### 5.1 Protected regeneration

`update` and `codegen` use one generator, ownership planner, and transaction
implementation. The only public-schema input difference is acquisition versus
the adopted payload. Both use current private SDL and configuration. `update`
already includes code generation; it never requires a following `codegen`.

Both operations verify the previous owned generation without requiring that
current local edits match it. In particular, jointly edited public and private
SDL may be incompatible with the old generation but valid together; `update`
must allow that combination. `codegen` instead checks private edits against the
old adopted public schema and fails if they require an unadopted public change.

Retain base/user/next comparison, `.dist` proposals, orphan handling, seed and
configuration preservation, active-file conflicts, and bounded staging.
Invalid old authored resolvers or seeds do not prevent placement of new valid
candidates. They still fail subsequent validation. Do not rewrite public SDL,
storage, authored configuration, or existing seed content.

Publish snapshot, declarations, candidates, and management information through
the existing single-generation transaction. Validation/planning failure leaves
the project unchanged. Installation failure follows existing rollback and
retained-recovery rules; do not claim power-loss atomicity or hide post-commit
cleanup failures. Project reference now also rejects an incomplete transaction.
Identical inputs produce no unnecessary bytes, proposals, or generation churn.

Resolver, hook-body, and seed edits alone need validation, not codegen.

### 5.2 Read-only public comparison

`update --check` verifies the adopted snapshot and owning metadata, reads and
normalizes the configured public SDL, and validates that public schema using
the existing vocabulary. It does not require current private SDL, regenerate
types, initialize modules, inspect seeds, or validate a runnable project.
Comparison can therefore succeed while local codegen or resolver work remains.

The result distinguishes `in_sync`, `update_required`, and `unavailable`.
Exit codes are respectively 0, 1, and 2 for this comparison operation.
Unavailable includes missing, invalid, unsupported, changed-during-read, or
over-budget comparison inputs. It never means synchronized. Other commands
retain their existing error categories and exit rules.

JSON uses the closed `chigiri-mock-schema-sync/v1` report with `format`,
`status`, `readiness_checked` (always false), `adopted_digest`, `input_digest`,
and `diagnostics`. Both digest fields are nullable; publish a digest only for a
successfully verified side. Conclusive results have both digests and no error
diagnostics; unavailable has a sanitized cause. Reuse existing bounded
diagnostic values, not raw SDL, description content, private values, or
physical paths. Text expresses the same result and next action. Invalid CLI
grammar still uses ordinary usage handling. Direct output retains existing
BrokenPipe handling; it is not evidence that a consumer received a full report.

The operation reports public freshness, not an update dry run or a guarantee
that adoption will succeed with current local inputs. It makes no filesystem
changes and never clears recovery materials.

## 6. Description-aware reference and export

Use the same semantic graph for reference contracts and descriptions. Retain
existing type, field, argument, input, enum-value, and directive descriptions;
add the missing nullable descriptions to model/model-slot and resolver topic
entries, derived from their actual schema declarations. Do not invent a
description for synthesized slots or infer business rules from prose.

Text and JSON must agree. Absent descriptions remain null in JSON and omitted
in text; an explicitly empty description remains an empty value. Preserve
existing source-role visibility: authoring model reference may describe private
storage where already permitted, but public type views, introspection, and
responses must not gain private fields or their descriptions. User SDL
descriptions are data, not instructions to execute or privileged tool guidance.
Retain bounded output and terminal-control/bidi escaping in text, and ordinary
JSON escaping without losing the decoded value.

`reference --schema` remains an explicit view of live source SDL, with
`input_kind = schema`; it never reads or updates a project. Bundle reference
uses embedded SDL, and common reference remains schema-independent. Project
reference text identifies the adopted-contract boundary; generated guidance
uses project reference for implementation and `update --check` for freshness.

Export embeds the adopted public SDL and current validated private SDL, with
descriptions and existing logical names, through the current portable bundle
contract. It never rereads the external public SDL, includes project management
metadata, or exports acquisition paths. Directory and exported-bundle schema,
description, and HTTP behavior must agree after the external inputs disappear.
No snapshot-inspection command or bundle-to-project importer is introduced.

## 7. Authoring workflow and consequences

The normal sequence is:

1. Initialize from explicit public SDL and open the project.
2. Edit private storage or generation-bound settings, then run `codegen`.
3. Read scoped project reference and implement resolvers, hooks, and seeds.
4. Use target checks during editing and full check before serve/export.
5. When choosing to adopt public changes, optionally run `update --check`, run
   `update`, review preserved edits and proposals, then validate again.

Update guidance, reference examples, and init-generated `AGENTS.md` at
implementation cutover. Do not tell users to inspect live external descriptions
as the authority for an older project. Do not overwrite existing user guidance
automatically or add completion-state bookkeeping removed by ADR-0051.

Intentional consequences are an additional explicit `codegen` command, a larger
but bounded snapshot, and a generation-consistency prerequisite for project
reference. In return, external API work cannot unexpectedly change or disable
a working Mock, and local storage work cannot accidentally adopt that API work.

Rejected alternatives are live-description lookup, two editable public schema
copies, one update operation for both purposes, a local-update flag, separate
adopted private/configuration copies, and hand-edited generated snapshots.
Shared Luau modules, upstream patches, background watchers, caches, a Mock test
runner, semantic rename inference, and new release infrastructure are not part
of this decision.

## 8. Version cutover and migration

| Surface | ADR-0051 baseline | New contract |
| --- | --- | --- |
| Project | `chigiri-mock-project/v7` | `chigiri-mock-project/v8` |
| Update state | `chigiri-mock-update-state/v8` | `chigiri-mock-update-state/v9` |
| Schema snapshot | `chigiri-mock-schema-snapshot/v8` | `chigiri-mock-schema-snapshot/v9` |
| Full reference | `chigiri-mock-reference/v10` | `chigiri-mock-reference/v11` |
| Topic reference | `chigiri-mock-reference-topic/v6` | `chigiri-mock-reference-topic/v7` |
| Public comparison report | Absent | `chigiri-mock-schema-sync/v1` |
| Normalization identity | Absent | `chigiri-mock-sdl-normalization/v1` |
| Bundle / host API / check | v9 / v8 / v3 | Unchanged |
| Capabilities / log / vocabulary / Rust ABI | v2 / v3 / v3 / ABI 6 | Unchanged |

Project and management changes identify the new authority and generation
contract. Reference changes identify description-bearing shapes and the project
input boundary. The existing bundle already carries complete SDL and can
preserve descriptions without a new shape; the resolver host API is unchanged.

Reject old or mixed project/management identities before writes. An old summary
cannot reconstruct the public SDL it originally described. Do not manufacture
that history by adopting today's external source under an automatic upgrade.
No compatibility reader, format-string-only conversion, or in-place migration
is provided. Initialize a separate absent destination from deliberately chosen
public SDL, copy/adapt private SDL into that directory and edit configuration,
run `codegen`, then deliberately transfer authored modules and seeds. Do not
copy old generated artifacts. Validate the new project before replacing any
existing workflow. Existing compatible bundle input remains supported.

## 9. Implementation packages and acceptance

Implement in this order, keeping each package reviewable:

1. AS-WP0: capture current behavior and the version/ownership inventory; add
   normalization and source-authority regression fixtures.
2. AS-WP1: add shared normalization, the owned public payload, separate hashes,
   local storage confinement, and internally consistent snapshot readers.
3. AS-WP2: connect protected init/update/codegen, read-only comparison, project
   checks/reference/runtime/export, and the coordinated format cutover.
4. AS-WP3: complete description projection, guidance, migration, and the focused
   CLI/HTTP acceptance below; record exact-source evidence and review findings.

All acceptance IDs start Required / Not run. Earlier validation results do not
establish these properties for the changed code.

| ID | Required assertion |
| --- | --- |
| AS-01 | Init stores bounded, logically named, description-preserving normalized public SDL in the existing snapshot, with separate verified public and internal identities. Generated artifacts are reproducible. |
| AS-02 | Whitespace, commas, ignored BOMs, line endings, comments, and equivalent string spellings do not cause drift. Meaningful description/value/directive/order changes do. Include strings containing `#`, quotes, escapes, multiline content, invalid syntax, idempotence, and reparse equivalence. |
| AS-03 | Changing, corrupting, moving, or deleting external public SDL does not affect valid project reference, full/target check, serve, or export. Verify that these paths are not opened; HTTP and introspection still use the adopted schema. |
| AS-04 | Local storage/configuration drift directs internal readers to codegen; codegen changes only the local generation and never opens public paths. Resolver/seed errors do not prevent generation or contract reference, but still fail applicable validation. |
| AS-05 | Update adopts descriptions and structural changes and generates in one transaction. Joint public/private edits can succeed without preceding codegen; codegen alone cannot adopt them. Repeated identical inputs are a no-op. |
| AS-06 | Both writers preserve edits, seed/config/storage content, proposals, and orphans. Test conflicts, cancellation/concurrent changes, pre-publication failure, rollback/recovery, and incomplete-generation rejection by reference as well as check/serve/export. |
| AS-07 | Real CLI comparison distinguishes all three states with exact text/JSON and exit behavior, makes no writes, and works with pending private/resolver work. Unavailable sources never report synchronized. Existing usage and direct-output safety remain intact. |
| AS-08 | Public descriptions agree across scoped text/JSON, full reference, introspection, and export. Model slots and resolver descriptions are present where declared; absent/empty values, private visibility, control characters, and aggregate output bounds are covered. |
| AS-09 | Reject absolute, traversal, symlink, duplicate/aliased storage inputs and snapshot/hash/summary/type corruption. Test raw/normalized/escaped size limits. Diagnostics refer to actual logical source representations, never fabricated original line positions or leaked physical paths. |
| AS-10 | Old/mixed project formats fail without writes; fresh-destination migration succeeds. A relocated internally complete project works without public inputs, and a portable bundle has equivalent schema and representative HTTP behavior. All documented new commands/examples have executable coverage. |

Use local macOS arm64 for this implementation assignment, focused unit and
real-CLI tests for the changed boundaries, representative HTTP/export cases,
affected-package quality checks, and repository implementation rules. Avoid
repeated full-product runs during iteration; any final broader run must be
recorded against its actual source and must not be claimed for later edits.
Remote Linux, cross-builds, publishing, and platform requalification are not
completion dependencies of this assignment. Product support tiers are not
changed by that evidence limit. No per-project business-test framework is
required. Mark the ADR implemented only after its acceptance and retained
safety checks are evidenced; publication of this document alone is not proof.
