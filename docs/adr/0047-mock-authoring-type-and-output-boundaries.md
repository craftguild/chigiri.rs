# ADR-0047: Mock authoring type and output boundaries

Status: Accepted

Date: 2026-09-22

Implementation: Implemented; AB-01 through AB-16 passed on final source

The implementation goal prompt
preserves the self-contained implementation instructions. The final evidence
map is the
Mock authoring boundaries validation record.

## 1. Context and evidence

A fresh authoring run against the ADR-0045/0046 baseline implemented the sibling
`graphql` project. Standard libraries, private storage, seed references,
session-based authentication, business rejection, and stateful mutations were
usable without the earlier fixed-user and missing-library workarounds. However,
successful full checks still preceded output failures, and ordinary model calls
still required explicit intermediate type annotations.

The following findings combine the supplied authoring transcript, source
inspection, and isolated probes against the current Mock library. They are
reproduction evidence, not passing acceptance evidence for this follow-up.
The sibling directory and temporary probes are not future build dependencies.

| Observation | Classification and evidence |
| --- | --- |
| Inline multi-key `orderBy` fails while a typed local succeeds. | Checker integration defect. Narrowing each key to its singleton does not supply the array's common expected element type; a two-key `rank`/`id` probe reproduces the failure. |
| A valid predicate or order works in a direct call but fails in `find(...)[1]`. | Checker integration defect. Model-call recognition misses calls followed by indexing. A single-key order and an enum equality predicate independently reproduce this. Constant bracket-key options also fail. |
| Private model numbers need `local balance: number` before comparison. | Generated-type defect, not an ordinary nullable refinement problem. Private slots occur in both `Stored` and the `Record` extension; the pinned checker rejects comparison of the resulting `number & number`. Removing the redundant extension in an isolated declaration probe makes the comparison pass. |
| Nested Wallet and PostImage values pass checks and writes, then fail during selected child output. | Output-boundary defect. An unchanged image record embedded in a new post reproduces `private output DTO field` for both an object and a list. This does not require modifying the returned post record. |
| Assigning personalized values or a refreshed wallet to a returned record fails. | Intended provenance protection with inadequate authoring guidance and diagnostics. A model record is not a freely editable response DTO. The successful switch to `@virtual` is appropriate for derived fields, but is not a necessary workaround for every embedded object. |
| Errors remain whole-module type mismatches or generic execution failures. | Diagnostic gap. Some precise checker sites and internal value-validation causes are lost; native field failures may name a resolver file that does not exist. Sanitized HTTP failures themselves are intentional. |
| `reference hooks request.before` fails; guessed runtime names lead to repeated discovery attempts. | Discovery gap. Named hook selection is excluded by the current topic contract. Add it explicitly and improve valid-name guidance rather than accepting arbitrary aliases. |

Relevant implementation boundaries are
[record declarations](../../crates/mock/src/luau/models.rs),
[model-call recognition](../../crates/mock/src/luau/literal_precision_syntax.rs),
[literal precision](../../crates/mock/src/luau/literal_precision.rs),
[checker diagnostics](../../crates/mock/src/luau/validation_batch.rs),
[output validation](../../crates/mock/src/output/validate.rs),
[native field execution](../../crates/mock/src/service/native.rs), and
[runtime diagnostics](../../crates/mock/src/service/hooks/diagnostics.rs).

## 2. Decision and scope

Complete the authoring and projection contracts already established by
[ADR-0045](0045-stateful-mock-authoring-and-runtime-contract-parity.md), without
relaxing provenance, public-schema secrecy, request atomicity, or resource
limits. This ADR adds named hook discovery and the diagnostic changes below;
it does not rewrite the earlier acceptance history. The
[ADR-0046](0046-standard-os-time-and-explicit-timeleap-control.md) `os` and
timeleap contracts remain unchanged.

Public SDL remains the sole definition of public field behavior in Pythia:
stored values, explicit `@relation`, and explicit `@virtual`. Storage SDL may
add private data but may not replace public fields or attach alternative public
resolver behavior. Do not add a mock-side override file, overlay, inference
rule, or automatic conversion from a stored field to a virtual field.

An ordinary unannotated, argumentless, non-root field is valid GraphQL; it
declares stored/value behavior in Pythia. Root fields and fields with arguments
retain their existing automatic resolver scaffolding under ADR-0038. If a
stored field does not express the application's intended behavior, the author
must change the source SDL with the schema owner's authorization and regenerate.
The checker must not infer business intent from names such as `viewerVote`,
`wallet`, or `images`.

This change records design only. Product implementation and the AB acceptance
program below are separate work; none of their results are implied by accepting
this document.

## 3. Generated types and contextual query checking

### 3.1 Compose every record slot once

Generate `Stored`, `Record`, concrete record, create, and update shapes from the
shared slot policies. A slot already represented by `Stored` must not be
redeclared through an intersection merely because it is private. Include real
record-only slots where needed and preserve requiredness, nullability,
represented scalars, enum alternatives, key immutability, and concrete-type
requirements. Do not repair the symptom by widening private fields to `any` or
requiring authors to copy numbers into annotated locals.

Public models extended with private fields and wholly private models must both
support direct numeric comparisons and arithmetic after ordinary nil checks.
This does not promise that every mutable nullable property remains narrowed
across calls: genuine Luau nullability/refinement errors remain errors.

### 3.2 Supply the expected query shape, not just singleton casts

The following ordinary forms must check without author-written casts, typed
intermediate locals, or wrapper functions. In this example, `Item` has `id: ID!`,
`rank: Int!`, and `status: Status!`, with `Status` containing `OPEN` and `CLOSED`.

```luau
local rows = ctx.models.Item:find({
    where = { all = {
        { field = "status", op = "eq", value = "OPEN" },
        { field = "rank", op = "gte", value = 0 },
    } },
    orderBy = {
        { field = "rank", direction = "desc" },
        { field = "id", direction = "asc" },
    },
})
local first = ctx.models.Item:find({
    where = { field = "status", op = "eq", value = "OPEN" },
})[1]
```

Derive expected options, predicate operands, and array element types from the
same model/operator definitions that drive runtime validation. In particular,
the order array needs its declared common element type, not the type inferred
from its first literal. Apply the rule to the existing model methods that
accept these query shapes, not only `find`.

Prefer corrected declarations and small checker-only contextual bindings over
additional bespoke inference rules. If an AST transformation supplies context,
it must introduce checked annotations or equivalent checked parameter
boundaries, not assertions that can suppress invalid input. Do not fork the
pinned checker or modify third-party sources under this work package.

Recognition must handle method calls within indexing and parentheses, constant
bracket keys, direct and bracket model access, and lexically proven aliases.
Use the shared literal decoder for escaped and long-bracket strings. Respect
shadowing, reassignment, loops, parallel assignment, and nested functions; never
rewrite an unrelated local method that happens to be called `find`. If the
model or key cannot be proven, retain ordinary checking instead of guessing.

Keep original runtime source and evaluation meaning unchanged. Preserve
single evaluation, call order, module isolation, generated-name hygiene,
source-span mapping at both ends, and existing transformation/work budgets.
Invalid fields, operators, operand types, enum members, directions, options,
nullability, and write scopes must still fail. Valid siblings must not hide an
invalid `limit`, and an error inside an IIFE must not be blamed on an enclosing
enum predicate.

## 4. Separate stored-value projection from arbitrary DTO validation

Use explicit host-owned origin information at the output boundary. An `inline`
field role alone is not proof of trusted origin. Distinguish these cases:

| Value origin | Internal meaning | Output and nested parent behavior |
| --- | --- | --- |
| Genuine unchanged model record returned through a Payload, edge, list, or other local container | Existing request/snapshot-bound provenance | Preserve record identity, private parent slots, and declared relations. |
| Embedded value read by the host from a validated stored slot | Typed stored snapshot, not an independent row | Project public fields and retain allowed private values internally for nested resolvers, without synthesizing record identity. |
| Arbitrary resolver-created DTO | Untrusted value matching an allowed output shape | Apply existing public DTO validation; private fields and forged authority remain invalid. |

Pass trusted stored-slot origin through native field reads, recursive embedded
objects, lists, and abstract concrete values. Validate against the combined
storage contract, then dispatch and serialize exclusively through the public
schema. Private fields remain available to the appropriate internal parent
contract but must never become selectable, introspectable, or automatically
logged. Private implementation types cannot serve as public concrete output.
Do not expose a Luau flag, magic key, or constructor that grants this origin.

Preserve the distinction when encoding a nested resolver's parent into Luau.
It must receive its stored private slots without being registered as a model
record or gaining a live relation from an `id` lookup. Copying that parent into
a response does not grant new output authority. Runtime and generated parent
types must describe the same accessible slots.

An image with private ownership metadata must therefore remain usable in a
stored `Post.images` list. Pythia must not require a private link model and a
virtual resolver merely to avoid rejection of that metadata. Authors may still
choose an explicit link for independent business reasons. The same projection
rule applies to seed literals, expanded seed `$ref` values, create inputs,
updates, restart, and portable export.

Persistence still copies values. Storing a genuine model record in an
unmarked object field does not persist its transient output provenance or
create a live relationship. Later changes to the source row do not refresh the
embedded snapshot. Seed `$ref` is value reuse, not live linking. Use an explicit
relation or a virtual field when current independent state is required.

Reject forged, modified, foreign, and stale authoritative records as before;
do not silently strip private fields to make a modified record acceptable or
rebind it to a current row. Reordering an ordinary array of unchanged records
and wrapping records in ordinary containers remain valid. Keep allocation,
depth, cumulative work, snapshot ownership, cancellation, and whole-request
rollback guarantees across all three paths.

## 5. Actionable diagnostics without value disclosure

### 5.1 Type checks

For covered model calls and generated shapes, report the affected member or
option, schema-derived expected type, safe actual category when known, and a
fixed corrective hint. Do not default to the entire module descriptor when a
more precise mismatch is established. Prefer a correct explicit unknown to an
invented actual type or a nearby but unrelated field attribution.

The pinned checker exposes source spans and rendered messages, not a complete
structured type-error tree. This ADR does not require reconstructing that tree
with message-string heuristics or publishing raw compiler text. Combine
compiler spans with proven AST/contract information, and retain the sanitized
fallback for errors that cannot be classified reliably. Test successful nearby
sites as well as failures to prevent misleading attribution.

### 5.2 Local runtime diagnostics

Carry fixed value/provenance failure classifications through the runtime and
native output paths instead of collapsing every failure to one invariant.
At minimum, distinguish private fields in an arbitrary DTO, invalid stored
value shape, modified/forged/stale record provenance, and output type or
required-field mismatch. Keep these diagnostics separate from `ctx.fail`.

The local resolver diagnostic retains request correlation, coordinate, phase,
cause, and bounded stack, and adds nullable string fields `member`, `expected`,
`actual`, and `hint`. These contain only schema-derived identifiers, safe structural
categories, and fixed host-owned wording. Unknown names or arbitrary table keys
must not be echoed as schema members. Check diagnostics retain their existing
v2 fields and apply the same sanitization policy.

Resolve module identity from the verified active script inventory, not by
inventing a file path from a GraphQL coordinate. A native stored/relation field
with no script reports `module`, `line`, and `column` as null. A real script
with no available position retains its logical module and null positions.
Mapped positions and stack frames refer only to real source locations; do not
use line 1 as an invented location for a native projection failure.

No automatic diagnostic includes input values, private values, tokens, request
bodies, source excerpts, raw exceptions, or physical paths. Preserve bounds,
deduplication, truncation signaling, and independent-failure precedence.
HTTP internal failures remain sanitized and correlated; their public payload
does not gain these local diagnostic details.

## 6. Discovery and authoring guidance

Support exact single-item hook selection for the six existing names:
`request.before`, `request.after`, `operation.before`, `operation.after`,
`field.before`, and `field.after`. Text and JSON selection use the same hook
registry; examples and capabilities must match the selected slot. Unknown
names give a bounded valid-name list or an exact working discovery command,
with existing usage exit status and no partial JSON on stdout.

Improve help and generated guidance so authors discover `reference runtime`
and its actual entries before requesting nonexistent `context` or `state`
topics. Do not retain invented topic names as aliases. Keep no-argument
reference output bounded and focused; full reference dumps are not a required
step in normal authoring.

Generated `AGENTS.md`, scoped references, and the project workflow must share
tested examples explaining:

- Stored fields are snapshots; relations are explicit; viewer-dependent values
  belong in virtual fields. Live Wallet lookup and stable embedded Image values
  illustrate different decisions, not two universal virtual-field recipes.
- Model records may be composed into responses unchanged. Persistence writes
  use model operations; response personalization must not mutate authoritative
  records. A DTO is not a model record with selected fields overwritten.
- `ctx.request` is already available to root and nested resolvers without
  enabling hooks. A request hook and declared state are optional tools for
  shared authentication preparation, not prerequisites for reading headers.
- Private fields/models, seed references, nullable values, and record/parent
  types follow the combined storage contract without changing public exposure.
- Public SDL edits require schema-owner authorization. If derived behavior is
  needed but edits are not authorized, report that boundary instead of silently
  changing SDL, weakening behavior, or adding a mock-side override. After an
  authorized edit, run canonical `update`; never patch generated declarations.
- Target checking, full readiness, and actual selected-field HTTP scenarios
  prove different things. Full check is not proof of business semantics.

Guidance must not encourage typed locals or unchecked casts as the standard
remedy for the inference defects in Section 3. Explicit annotations remain
valid authoring choices, and genuine nullable-property errors may still need
ordinary refinement or a local value.

## 7. Versioning and delivery boundaries

Use one current contract per surface, with no compatibility reader, alias,
dual output mode, or silent legacy adaptation. At implementation cutover:

| Surface | Baseline | Follow-up |
| --- | --- | --- |
| Runtime logs | `pythia-mock-log/v2` | v3: precise local details and nullable source positions. |
| Full/topic reference | v8 / v4 | v9 / v5: named hook selection and the updated logging contract. |
| Check diagnostics | v2 | v2: existing shape, more precise safe contents. |
| Project / bundle / host API / capabilities | v6 / v8 / v7 / v2 | Unchanged: no new input syntax, host function, or capability. |
| Update state / schema snapshot | v7 / v7 | Unchanged shape; recompute generated identity through canonical generation. |

Update reference readers, selection validation, help, exact fixtures, generated
declarations, and the log contract together. Old generated bytes must not be
accepted under an identity that describes the corrected declarations; use the
existing canonical update and ownership checks, preserving user sources,
`.dist`, unmanaged files, and recovery protections. No author-source rewrite is
automatic. If another change consumes a planned version, reconcile the version
inventory before implementation rather than reusing it.

Generated Rust ABI 6, public GraphQL validation, normal-product and Mock MSRVs,
standard-library/timeleap behavior, and existing resource ceilings do not change.
Historical ADR-0045/0046 validation records remain historical; do not mark this
follow-up implemented by citing their test counts.

## 8. Implementation sequence and acceptance

Implement in reviewable packages: AB-WP0 establishes self-contained baseline
fixtures and the evidence map; AB-WP1 fixes declaration composition and query
context; AB-WP2 separates trusted stored projection and nested parents;
AB-WP3 delivers diagnostics, discovery, guidance, and version changes; AB-WP4
runs integrated acceptance and records final-source evidence. Run the
implementation-rules checker before work, after each package, after formatting,
and immediately before handoff. Follow the normal implementation and validation
rules; do not import a separate historical goal's completion record.

All entries below passed on the final implementation source. Each fixture
asserts values or precise rejection behavior, not just successful compilation,
HTTP status, or an `id`-only selection.

| ID | Required evidence |
| --- | --- |
| AB-01 | Public and private models accept valid inline predicates and heterogeneous multi-key ordering without annotated locals; invalid fields, operators, operands, enums, directions, options, and scopes remain rejected across applicable model methods. |
| AB-02 | Calls followed by indexing or parentheses, direct/bracket model access, constant bracket options, escaped/long-bracket strings, and proven aliases receive equivalent checking. Shadowing, reassignment, loops, parallel assignment, and unrelated local calls do not acquire host rewrites. |
| AB-03 | Private-model and private-extension numbers support comparison/arithmetic after nil checks without redundant intersections. Nullable, represented-scalar, enum, required-field, key, and concrete-record constraints remain intact. |
| AB-04 | Wrong operands, directions, limits, and returns receive precise safe diagnostics where provable; unknown cases remain honest. Both span endpoints map to original source. IIFE errors and valid neighboring fields do not produce false attribution. |
| AB-05 | Embedded public objects and lists with private extensions work from literal seeds, seed refs, create, and update through actual HTTP child selection; nullable, multi-level, and public abstract-type cases are covered. |
| AB-06 | Nested virtual resolvers can read permitted private stored parent values. Public validation, introspection, serialization, and automatic diagnostics reveal no private values or private concrete types. Arbitrary DTOs cannot claim stored origin. |
| AB-07 | Updating an independently stored image leaves its embedded snapshot unchanged; explicit relations observe the appropriate current snapshot. Persistence and seed refs do not promote embedded values to records. |
| AB-08 | Unchanged records survive Payload/list/Connection composition, including `feed.edges.node.reward` and `wallet.rewards.edges.node.post` equivalents. Modified, forged, foreign, and stale records fail without implicit rebinding; ordinary list reordering succeeds. |
| AB-09 | Native output failures have safe specific causes and no invented module/position. Script failures retain actual logical sources. Local v3 logs and check v2 are bounded and secret-free; HTTP internal failures remain sanitized and correlated. |
| AB-10 | Projection/provenance failure after earlier writes rolls back the entire request; later-root/after-hook failure, cancellation, limits, and VM recovery retain their guarantees. A subsequent healthy request succeeds. |
| AB-11 | All six named hook references work in text/JSON and share list/full definitions. Unknown topic/name and unsupported selections return actionable bounded guidance with correct exit/stdout behavior. |
| AB-12 | Generated guidance and reference examples are executed, including direct request access with hooks disabled, optional request-state preparation, stored images, virtual viewer values, live wallet lookup, and source ownership/update rules. No public-field override surface is introduced. |
| AB-13 | A fresh init/storage/update/seed/resolver workflow exercises authentication switching, ownership, virtual viewer values, balance changes, idempotent transfer, nested relations, and embedded images by HTTP. Target/full readiness, restart, and portable export agree. |
| AB-14 | A renamed or second domain reproduces the typing, private embedding, provenance, and discovery guarantees without User/Post/Wallet name assumptions. Tests depend only on repository-owned fixtures, not `../graphql`. |
| AB-15 | Current-format update preserves user-owned files and recovery rules while regenerating corrected types and identity. Reference v9/topic v5 and log v3 fixtures are exact; obsolete reference documents are rejected and no compatibility path is introduced. |
| AB-16 | Focused and full Mock/CLI/HTTP tests, applicable debug/release recovery tests, formatting, Clippy, rustdoc, generated-contract checks, dependency checks, implementation rules, and the repository validation gates pass on final source; unexecuted environments and unrelated failures are recorded separately. |

Keep a final evidence map from each AB-ID to commands/tests and source identity.
Add regression fixtures before the corresponding fixes, including failures
that only appear when nested fields are selected. A passing root execution is
not sufficient evidence for stored-value projection.

## 9. Non-goals and evaluation limits

This ADR does not add module loading, a general ORM/query builder, automatic
authentication, background reward settlement, real image upload handling,
unbounded aggregate queries, new time APIs, or relaxed execution budgets.

Repeated authentication helpers, `count() + 1` identifiers, and fetching only
the first 1,000 rows before pagination/aggregation are authoring pitfalls, not
evidence that those new product features are necessary. Examples should use
existing automatic IDs where eligible and explicit bounded query/pagination
strategies without claiming a truncated result is complete. A settled demo
reward that cannot be voted on is a seed/documentation inconsistency, not a
reason to introduce a background worker.

The two supplied authoring runs are not comparable elapsed-time benchmarks:
their prompts, seed volumes, achieved semantics, and execution records differ.
Record fewer semantic workarounds as qualitative progress, not a measured
speedup. Future comparisons must fix the starting SDL/configuration, binary
identity, task, seed requirements, agent setup, acceptance scenarios, and
completion definition; report wall time, tool/check retries, runtime failures,
schema changes, and remaining compromises separately. Running such a benchmark
is not part of this documentation change or a substitute for AB acceptance.
