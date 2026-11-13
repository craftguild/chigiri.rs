# ADR-0045: Stateful mock authoring and runtime contract parity

Status: Accepted

Date: 2026-09-19

Implementation: Implemented

## 1. Context and evidence

An external agent used the ADR-0044 workflow to implement a generated project
with 22 resolvers, 15 main seed records, and authentication, voting, rewards,
image metadata, and transfers. Scoped references and target checks supported
incremental development, and the project eventually passed full readiness.
HTTP requests nevertheless exposed runtime failures and encouraged workarounds
that weakened the intended behavior.

The authoring log and read-only investigation of the sibling `graphql/schema`
project identify the following problems. They are design evidence, not a record
of passing acceptance tests for this ADR.

| Observation | Finding and required improvement |
| --- | --- |
| `table.sort`, `table.insert`, `math.min`, and `string.sub` passed type checking but failed at execution. | The checker exposes built-ins that the runtime global allowlist removes. This does not establish that `find()` results are inherently unsortable. |
| Inline `orderBy` and enum predicates required casts or rewrites. | Luau literal inference and generated API types disagree in ordinary calls. These are not missing Rust methods. |
| `post(id).reward` worked, but `feed.edges.node.reward` and `wallet.rewards.edges.node.post` failed. | Output validation treats nested stored objects as DTOs and loses the model context needed for relations. |
| Login selected the first Viewer; signup stored a proof token as a phone number. | HTTP success masked semantic compromises. Public schema fields and synthesized relation keys are insufficient as a general internal storage contract. |
| Ownership, upload state, votes, and idempotency information were encoded into IDs or omitted. | Mock authors need typed private fields and private models without changing the public API. |
| Dates were hard-coded, and complete User values were repeated in seeds. | A controllable application clock and explicit seed-value reuse are missing. |
| Type and runtime failures were mostly generic. | Useful checker information is discarded; existing bounded stack machinery does not make all failure paths actionable. |

Relevant implementation entry points include
[sandbox registration](../../crates/mock/src/sandbox.rs),
[type validation](../../crates/mock/src/luau/validation_batch.rs),
[resolver context](../../crates/mock/src/runtime/context.rs),
[output validation](../../crates/mock/src/output/validate.rs), and
[seed relation normalization](../../crates/mock/src/store/seed_relations.rs).
The sibling project is not a future build or CI dependency.

## 2. Decision and boundaries

Make stateful mock behavior practical without weakening type safety, public
schema boundaries, resource limits, or request-atomic execution. Extend the
deterministic authoring primitives from
[ADR-0044](0044-composable-agent-ready-mock-authoring.md); do not restore an
embedded agent, inference service, or interaction UI.

This ADR changes the following Mock contracts:

- ADR-0029/0032: the minimal VM environment, project configuration, and export.
- ADR-0036: request access, diagnostics, and the explicit rejection surface.
- ADR-0038: the exclusively public-schema-derived model inventory and nested
  record output handling. Ordinary embedded storage remains value storage.
- ADR-0039: add an explicit business rejection source while preserving whole
  request rollback, failure precedence, and commit fencing.
- ADR-0041/0044: capability discovery, structured diagnostics, generated
  guidance, and their versioned output contracts.
- ADR-0042: preserve batch isolation and source mapping, but allow the checker
  integration changes necessary for runtime parity and literal inference.

Public Rust generated ABI 6, the normal framework execution policy, existing
MSRV boundaries, and repository implementation rules remain unchanged.
The current Mock binary implements this contract. Section 12 defines the
acceptance evidence that must remain green when the surface changes.

## 3. One executable capability contract

Use one typed capability registry for runtime registration, checker globals,
generated declarations, and reference output. Share it across configuration
evaluation, module initialization, hooks, and resolvers, with explicit scope
restrictions for context APIs.

The initial standard-function set includes the existing `assert`, `error`,
`pcall`, `type`, `pairs`, and `ipairs`, plus:

| Group | Functions |
| --- | --- |
| Conversion | `tostring`, `tonumber` |
| String | `string.len`, `string.sub`, `string.find`, `string.lower`, `string.upper` |
| Math | `math.abs`, `math.ceil`, `math.floor`, `math.min`, `math.max` |
| Table | `table.insert`, `table.remove`, `table.sort`, `table.concat`, `table.clone` |

Published signatures, overloads, and return values must agree with execution.
String method syntax exposes only the same allowed string operations. Library
tables and their metatable paths cannot expose additional capabilities.
Do not enable all VM libraries as a shortcut: OS, filesystem, network, process,
dynamic loading, environment mutation, and debug access remain unavailable.

Static references to unavailable globals or library members fail checking;
dynamic access through weakened types cannot enable them at runtime. Ordinary
local variables may shadow global names. `table.sort` works on ordinary local
sequences, including mutable record-copy sequences returned by `find()`;
deep-readonly request and state values remain protected.

Native functions, pattern matching, allocation, and sort callbacks remain
subject to memory, cumulative work, deadline, cancellation, and reentry limits.
Do not assume that bytecode interrupts bound every native call. Use bounded
implementations or wrappers where needed and prove termination with external
watchdogs. Failures remain sticky, and the next VM must remain usable.

The checker must accept ordinary inline `orderBy` keys and enum predicates
without casts to `any` or wider string types. Improve generated type shapes or
the pinned checker integration rather than documenting casts as the primary
workflow. Invalid fields, operators, enum values, nullability, and write scopes
must still fail. Preserve ADR-0042 module isolation and accurate source spans.

## 4. Readonly request context

Add `ctx.request` to Query, Mutation, and nested resolver contexts, independent
of whether hooks are enabled. It uses the same bounded snapshot as hooks:
`method`, `host`, `port`, `url`, `body`, and `headers`.

- `host` and `port` describe the bound listener; `url` is the path and query,
  not a synthesized absolute URL or a trusted Host header.
- Headers remain an ordered collection of name/value entries that preserves
  duplicates and the existing reversible non-UTF-8/backslash representation.
- The snapshot and nested containers are readonly. All phases of one request
  see the same input; different requests cannot share mutable request data.
- Non-HTTP execution uses `ctx.null`, represented in the generated type.
- Snapshot construction and VM copies are charged to the existing budgets.
  Hooks being disabled must not disable bounds or cause silent truncation.

Request access does not authenticate a caller. User code resolves sessions and
ownership through its models. Do not automatically log headers, bodies,
credentials, or derived identity. Request metadata and request-local `state`
are not substitutes for persistent-in-process private models.

## 5. Preserve record identity through output containers

An unchanged record obtained from a model operation remains a model-backed
output value when placed inside a locally constructed Payload, Connection,
edge, list, or other output container. Its relations and nested resolver parent
use the appropriate request snapshot, including the existing Mutation candidate
snapshot rules. Generated result types must admit these record values without
requiring authors to remove hidden foreign keys manually.

Carry host-controlled record provenance through the Luau codec and output
validation. A table with the same `id` or `__typename` is not sufficient proof of
identity. Reject forged, foreign, or modified record values instead of silently
rebinding them to the latest row. Wrapping a genuine record in an ordinary
container must not erase its provenance.

This is an output composition rule, not a change to persistence semantics.
Unmarked object fields saved by seed/create/update remain embedded values;
explicit `@relation` declarations still define live storage relationships.
Plain DTOs remain valid where their value shape is permitted, but do not gain
relations through an implicit ID lookup. Do not require changes to the public
Connection SDL merely to return existing records from a resolver.

## 6. Separate public SDL from private storage SDL

### 6.1 Authoring surface

Add a separate, explicitly configured source list:

```luau
storage = { sources = { "storage.graphql" } },
```

Public `schema.sources` remains the public GraphQL contract. Storage sources
use ordinary SDL definitions and extensions, not a new Luau schema DSL or a
`@private` annotation on the public API. For example, when the public schema
defines `Session`, an author can write:

```graphql
extend type Session {
  userId: ID!
}

type MockVote {
  id: ID!
  userId: ID!
  postId: ID!
  value: MockVoteValue!
}

enum MockVoteValue {
  UPVOTE
  DOWNVOTE
}
```

The first version supports private object, enum, and represented scalar
definitions, and stored-field extensions of public or private objects. Reuse
existing `@relation`, `@constraint`, and `@representation` semantics. Storage
SDL does not declare operation roots, executable resolver fields, or new
private interfaces/unions. References to existing public types retain their
established value and relation rules.

Every declaration or field introduced by storage sources is storage-only.
Reject duplicate names, public field replacement, extensions of operation roots,
and public references to private definitions. Public SDL must validate without
storage sources. No private field or type appears in GraphQL validation,
introspection, output selection, or the normal generated Rust contract.

### 6.2 Shared storage contract

Compile a public schema and a visibility-aware storage contract from shared
semantic definitions. Do not create two independently maintained descriptions
of the same public field. Model records include applicable private slots;
public output projection uses only the public schema.

Expose private models through the same `ctx.models` API and eligibility rules
as public models. Private fields support the existing applicable query,
ordering, constraint, and relation operations. Only Mutation root execution can
write; Query, nested resolvers, and hooks remain readonly.

Include private records, keys, indexes, relation edges, and capacity accounting
in the existing request candidate and commit/rollback unit. Do not introduce a
second mutable global store. Seed validation, generated declarations, full and
target checks, schema/config digests, snapshots, update, and portable export
must all account for private storage. Missing required private fields are real
seed/write failures, not values that Pythia guesses from business names.

`reference types` describes the public GraphQL schema. `reference models`
includes private models and slots, their visibility, source ownership, and
self-contained type/key/relation information. Resolver references describe the
actual model permissions available at their execution scope. Reference output
does not include stored values or session credentials.

### 6.3 Low-learning-cost scaffolding and ownership

`init` creates an editable `storage.graphql` and its source registration. The
initial file contains commented examples, not active sample models. An empty
or comment-only storage file means no additions and is valid. An explicitly
empty storage source list is also valid. Paths use the existing bounded,
explicit source-loading rules; there is no implicit directory scan.

The examples explain adding a private key to an existing type and defining an
independent private model. Provide a copyable SDL/seed/resolver example set in
the runtime reference and test that same set; users must not learn by copying
unverified pseudocode. Examples state required public types rather than
assuming that every user schema contains a Session or User.

Generated `AGENTS.md` distinguishes editable private SDL from protected public
SDL and generated files. Explain the sequence: edit storage SDL, run `update`,
inspect model/resolver references, implement seeds and resolvers, run target
checks, then full check and HTTP scenarios. Regeneration uses the tool; it does
not authorize hand-editing generated declarations.

Private SDL is user-owned after creation. Updates do not replace its contents,
activate samples, rewrite public SDL, or overwrite unrelated files. Preserve
current-format user edits and `.dist`/recovery protections. Internal models do
not require new GraphQL resolver scaffolds merely because they exist.

## 7. An application clock, separate from execution deadlines

Project configuration selects one of these closed forms:

```luau
clock = { mode = "system" },
```

Alternatively:

```luau
clock = { mode = "fixed", now = "2026-09-19T00:00:00.000Z" },
```

`init` selects system mode. Capture one instant when the request execution
context is created, before its hooks and resolvers. Every phase and parallel
field in that request observes it through the readonly `ctx.clock` API:

| Function | Result |
| --- | --- |
| `now()` | Captured UTC Unix epoch milliseconds. |
| `parse(text)` | Parse a timestamp with an explicit offset and millisecond precision into epoch milliseconds. |
| `format(epoch_ms)` | Canonical UTC `YYYY-MM-DDTHH:mm:ss.sssZ` text. |
| `add(epoch_ms, delta_ms)` | Checked millisecond addition. |

Numeric arguments/results are finite exact integers in Luau's safe numeric
range, with timestamps restricted to calendar years 0001 through 9999.
Parsing accepts RFC 3339 date-time syntax with zero to three fractional digits;
reject invalid dates, leap seconds, missing offsets, excess precision, and
overflow rather than normalizing or silently rounding them. Fixed configuration
uses the canonical UTC representation. These helpers do not infer semantics
from a GraphQL scalar's name.

Fixed mode returns the configured instant on every request and survives export.
The clock does not depend on enabled hooks, mutate state, or grant access to
`os`. Non-HTTP execution also receives an execution-context clock. Configuration
evaluation and module initialization do not receive an application clock.

Use an injectable host clock to test progression and expiration boundaries.
The initial release adds no live clock-control endpoint, resolver-side clock
mutation, sleep API, scheduler, or automatic reward settlement. Real monotonic
deadlines, cancellation, and access-log timing remain independent of this clock.
Fixed time alone does not promise deterministic automatically generated IDs.

## 8. Seed-value references without implicit live relations

Add an explicit reference form in object-valued stored fields and their list
elements. Use a named key rather than a slash-separated pseudo-path:

```json
{"author":{"$ref":{"model":"User","key":{"id":"user-akari"}}}}
```

The key must identify a declared unique key of the named model, including all
components of a composite key. Do not resolve arbitrary predicates, hidden row
ordinals, ambiguous matches, URLs, or filesystem paths. The referenced model
must be compatible with the destination value type.

Resolve references at seed initialization and project the referenced record to
the destination's declared stored-value shape. Do not copy computed relation
fields or storage-only slots that the destination shape does not admit. The
result is an embedded snapshot, not a live link to future record updates.
Existing explicit `@relation` key references retain their separate meaning.

The marker is a closed object with no sibling overrides and is not interpreted
inside arbitrary JSON-valued scalars. Source order does not affect resolution.
Detect missing targets, malformed keys, type mismatches, and expansion cycles
before serving. Cyclic expansion is invalid even when a separate relation
graph would legally contain a cycle.

Charge expanded values, reference traversal, fan-out, depth, and bytes to the
existing seed/value budgets; a compact input must not enable unbounded output.
Diagnostics identify logical file/record/field locations without echoing seed
values. Target seed validation includes its reference dependencies in addition
to existing relation components. Export contains validated resolved seed values
and never needs the original source directory to resolve them at runtime.

## 9. Explicit business failures, not leaked runtime exceptions

Add a resolver-only `ctx.fail({code = "INSUFFICIENT_BALANCE", message = "..."})`
operation with a non-returning generated type. It is available to Query,
Mutation, and nested resolvers, including readonly execution. Hooks retain
their existing `ctx.reject` API with numeric HTTP status; do not overload that
numeric `code` with a second meaning.

The payload is closed. `code` is required, matches `[A-Z][A-Z0-9_]{0,63}`, and
cannot use the reserved `MOCK_` prefix. `message` is optional, defaults to a fixed
safe message, and uses the existing explicit rejection message byte bound.
Both are deliberate public response content. Arbitrary extensions, headers,
stacks, and raw exception objects are not accepted.

A valid call marks a sticky business rejection. Its response is HTTP 200 with
`data:null`, an `errors` array, the supplied `extensions.code`, and host-owned
request ID and field path. No candidate changes or partial results commit,
even if Luau catches the call with `pcall` or an earlier root already wrote.

Invalid payloads are internal failures. Ordinary `error(...)` remains an
internal failure; do not parse message prefixes as error codes. Independent
VM/store/output failures, hard limits, and timeouts retain their precedence
over business rejection. When combined with hook rejection, use the existing
status aggregation with business rejection contributing status 200, without
changing the all-or-nothing commit decision.

## 10. Actionable diagnostics and scoped discovery

Type diagnostics carry a stable cause code, logical source position, affected
contract member, normalized expected/actual types, and a bounded corrective
hint. Runtime diagnostics carry a stable cause classification, logical module,
coordinate, position, bounded stack frames, and request correlation. Reuse and
complete the existing stack capture machinery rather than claiming it does
not exist. Preserve bundle TOML positions and separate embedded Luau positions.

Expected/actual types are structural descriptions, not arbitrary compiler
message strings. Schema-declared names and enum alternatives may be shown;
source literal values, credentials, request/state/seed values, absolute paths,
and VM addresses may not. Obtain structured checker/runtime causes or construct
fixed safe diagnostics; blindly forwarding or heuristically scrubbing raw
exception text is not the contract. If stack or cause detail is unavailable,
report that explicitly instead of fabricating frames or leaking raw messages.

Public internal-failure responses remain sanitized and correlate with local
diagnostics through the request ID. Explicit business messages in Section 9
are a separate opt-in publication path, not permission to log request data.
Keep text/JSON parity, terminal-control escaping, output bounds, truncation
indicators, and broken-pipe success.

Add `reference runtime [NAME]`, with exact selections `stdlib`, `request`,
`clock`, `errors`, and `storage`. It lists supported signatures, availability,
limits, and executable examples without requiring readiness. Existing model
and resolver topics remain the authority for schema-specific operations.
All scoped JSON remains self-contained. Teach exact hook names and distinguish
an unsupported method/operator from a type-inference failure.

Neither target nor full check executes arbitrary resolver business paths.
Target checks always report `readiness_checked=false`; only successful full
checks authorize serving/export. Storage SDL is a structural input, not a new
`--target` kind. Full and target validation use the same combined contract;
invalid structural inputs and resource failures never become target success.

## 11. Clean break and versioning

Pythia Mock has not been released to production. Implementation simplicity takes
priority: backward compatibility with the previous Mock contract is explicitly
out of scope, not a conditional optimization. Ship only the new contract. Do
not implement old readers, compatibility aliases or adapters, dual execution
modes, fallback inference, or automatic old-format conversion. Old Mock API
shapes, generated declarations, diagnostics, and persisted formats are not
supported inputs to the new implementation. Retained behavior below belongs to
the new contract; it does not require a legacy compatibility path.

| Surface | Current baseline | Target |
| --- | --- | --- |
| Project | `pythia-mock-project/v4` | `pythia-mock-project/v5` |
| Portable bundle | `pythia-mock/v6` | `pythia-mock/v7` |
| Host API | `pythia-mock-api/v5` | `pythia-mock-api/v6` |
| Full reference | `pythia-mock-reference/v6` | `pythia-mock-reference/v7` |
| Topic reference | `pythia-mock-reference-topic/v2` | `pythia-mock-reference-topic/v3` |
| Update state | `pythia-mock-update-state/v5` | `pythia-mock-update-state/v6` |
| Schema snapshot | `pythia-mock-schema-snapshot/v5` | `pythia-mock-schema-snapshot/v6` |
| Check diagnostics | `pythia-mock-check/v1` | `pythia-mock-check/v2` |
| Runtime logs | `pythia-mock-log/v1` | `pythia-mock-log/v2` |

Bind both public and private schema inputs, clock settings, generated types,
and capability versions into the applicable configuration/generation identity.
Export embeds both schema roles and clock settings. Reject unsupported or mixed
formats explicitly before mutation; do not claim readiness for stale metadata.
The normal generated Rust ABI stays at 6.

Compatibility and ownership protection are different requirements. Recognizing
an unsupported format to reject it is not a compatibility reader. A rejected
old project remains untouched. The only supported transition is a fresh
directory created by the new binary, followed by explicit adaptation/copying of
user SDL, seeds, hooks, and resolver logic. Do not copy old generated metadata
into the new project, automatically rewrite an old directory, delete `.dist`/recovery
material, or overwrite modified/unmanaged files. Keep current-format updates
transactional and document this manual transition clearly.

## 12. Delivery and acceptance

Implement in cohesive stages: capability/type/diagnostic parity; request access
and nested record provenance; private storage and scaffolding; clock, seed
references, and business rejection; then integrated version transition and
scenario verification. Each stage must retain the existing security and
ownership gates; intermediate work is not completion of this ADR.

| ID | Required acceptance evidence |
| --- | --- |
| SA-01 | Every published standard function passes generated-type checks and real execution; unavailable capabilities fail statically and remain unavailable dynamically. |
| SA-02 | Native work, patterns, allocation, and sort callback/reentry failures obey limits under watchdogs; cancellation and timeout do not prevent the next VM from succeeding. |
| SA-03 | Natural inline ordering and enum predicates pass without casts; invalid fields, operators, values, nullability, and write scopes still fail. |
| SA-04 | Hooks-disabled request access works for root and nested resolvers; duplicate/non-UTF-8 headers, bounds, readonly mutation, non-HTTP null, and request isolation are verified. |
| SA-05 | Both failing Connection/relation shapes from the authoring exercise work without public SDL changes; forged, modified, foreign, and stale record cases fail safely. |
| SA-06 | Private fields/models support CRUD, keys, relations, and constraints without appearing in public validation/introspection/output; no cross-request candidate leakage occurs. |
| SA-07 | Empty private SDL, commented scaffolds, copyable examples, update ownership, generated guidance, and model/resolver references form a tested onboarding path. |
| SA-08 | System/fixed clocks agree within a request; parsing, arithmetic, overflow, expiration boundaries, export, and independence from real execution deadlines are tested. |
| SA-09 | Seed references handle forward/composite-key references and deterministic expansion; reject missing/ambiguous targets, cycles, type errors, and excessive fan-out; JSON scalar markers remain literal data. |
| SA-10 | Target seed dependency checks and exported resolved seeds agree with full project behavior without reading sibling directories. |
| SA-11 | Business rejection exposes only explicit code/message, includes host correlation, rolls back all roots, survives `pcall`, and cannot mask an independent internal/resource failure. |
| SA-12 | Type/runtime diagnostics explain representative failures with correct source mapping and bounded stacks; secrets, source values, physical paths, and terminal controls do not leak. |
| SA-13 | Two users authenticate to distinct sessions; ownership checks and verification purpose, phone mapping, expiry, and one-time consumption hold without ID encoding or fixed-user shortcuts. |
| SA-14 | Upload metadata retains owner/declared size/completion state; votes are user-specific; no real upload service is required. |
| SA-15 | Transfer retries are idempotent; conflicting keys fail; balances, transfer records, and history either all commit or all roll back, including later-root/after-hook failures. |
| SA-16 | Fresh init through private SDL editing, update, target/full check, HTTP execution, and portable export succeeds with assertions on values and state transitions, not only HTTP status. |
| SA-17 | Old/mixed formats are rejected without changing user files; no legacy reader, alias, adapter, or automatic migration remains active; fresh-directory transition and current-format `.dist`/recovery protections are verified. |
| SA-18 | Repository rules, formatting, Clippy, rustdoc, focused/full tests, exact generated contracts, dependency checks, and existing VM recovery/ABI gates pass on the final source. |

Keep fixtures self-contained in this repository and cover at least two domains
or a renamed schema to detect business-name special cases. Authoring examples
are executable test inputs, not a second handwritten contract. Record actual
commands, outcomes, limits, and unexecuted environments separately; do not mark
this ADR implemented based on past test counts or the sibling project's HTTP
successes.

## 13. Consequences and non-goals

The design increases Mock IR, codec, generation, and validation responsibilities
but removes the need to weaken business behavior to fit the public schema.
Private SDL adds one concept; familiar syntax, inert scaffolds, executable
examples, and scoped discovery are required to keep its learning cost small.

Do not add a general ORM, permanent database, authentication provider, automatic
business resolvers, real image upload service, live clock-control service, or
production security claims. Existing scalar representations and explicit
relations are reused rather than replaced with a second schema language.

Rejected alternatives include keeping checker-only standard libraries,
teaching casts and fixed-user shortcuts as the normal solution, publishing
internal fields in public SDL, putting a large model DSL into `mock.luau`,
implicitly converting every embedded object into a relation, exposing raw
exceptions, and maintaining old compatibility layers for an unreleased Mock.
