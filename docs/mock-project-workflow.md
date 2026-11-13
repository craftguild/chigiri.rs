# Mock project workflow

This guide describes the current project format and operational workflow for
`chigiri mock`. The governing contracts are
[ADR-0044](adr/0044-composable-agent-ready-mock-authoring.md),
[ADR-0045](adr/0045-stateful-mock-authoring-and-runtime-contract-parity.md),
[ADR-0046](adr/0046-standard-os-time-and-explicit-timeleap-control.md), and
[ADR-0047](adr/0047-mock-authoring-type-and-output-boundaries.md), and
[ADR-0049](adr/0049-mock-authoring-modules-and-feedback.md), as superseded by
[ADR-0051](adr/0051-mock-contract-validation-without-completion-state.md) and
[ADR-0052](adr/0052-adopted-schema-snapshots-and-mock-codegen.md) and
[ADR-0054](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md).
Earlier inference and TUI workflows are historical and are not supported.

## Supported versions

The current clean-cutover formats are:

- project `chigiri-mock-project/v10`;
- portable bundle v11;
- host API v10 and capability registry v4;
- full reference JSON v13 and topic reference JSON v9;
- check JSON v3;
- runtime JSON Lines log v5;
- update state and schema snapshot v11;
- public comparison `chigiri-mock-schema-sync/v1` and SDL normalization
  `chigiri-mock-sdl-normalization/v1`; and
- Mock vocabulary v3.

Old layouts, aliases, readers and automatic migrations are rejected, including
the former protected v8 `mock update` exception retired by ADR-0057.
Older, mixed or unverifiable inputs require a fresh destination and deliberate
source adaptation below. No update command rewrites old configuration.

Framework libraries, generated SDKs, Mock and CLI use the fixed Rust 1.95.0
toolchain and minimum Rust 1.95 under [ADR-0059](adr/0059-craftguild-standards-adoption.md).

## Build and run the maintained quick start

Prepare the pinned engine and locked prerequisites from
[Building Chigiri](building.md#native-builds), then run from the repository root:

```sh
make debug
export CHIGIRI="$PWD/target/$(rustc +1.95.0 -vV | sed -n 's/^host: //p')/debug/chigiri"
python3 scripts/mock-quickstart.py --binary "$CHIGIRI"
```

The quick start exercises public freshness comparison, local code generation,
untouched scaffold check/export, removed-option rejection, resolver editing,
authored-body validation, a deliberate description-only public adoption, full
check, text and JSON reference, portable export, and real HTTP create/read
behavior.

For a project you control, initialize an absent directory:

```sh
"$CHIGIRI" mock init \
  --schema crates/mock/fixtures/blog.graphql \
  --config-dir target/blog-project
```

`init` never overwrites an existing destination. Repeating `--schema` adds
sources in declaration order. The generated root `AGENTS.md` explains which
files an agent may edit and which files are generator-owned.

## Project layout and ownership

The active project contains:

- `mock.luau`, which returns the closed project configuration;
- public schema acquisition locators and an optional private `storage.graphql`;
- role-specific resolver modules under `resolvers/`;
- model-specific JSON Lines seeds under `seeds/`;
- six optional before/after hook modules under `hooks/`;
- generated type declarations; and
- generated update-state metadata and a schema snapshot containing the adopted
  normalized public SDL.

Edit active resolvers, seeds, enabled hooks, `mock.luau`, and private storage
SDL. Do not hand-edit generated declarations, snapshots, or ownership records.
Full `check` establishes contract readiness; a successful scoped check is only
an incremental authoring result. Neither result claims business completeness.

An initial resolver module has a typed body that deliberately reports
`NOT_IMPLEMENTED`. Keep the generated function signature and replace that body
when the operation needs business behavior. For a schema containing
`type Note { id: ID!, title: String! }` and
`type Query { notes: [Note!]! }`, a completed example is:

```luau
--!strict
return {
    resolve = function(
        ctx: ChigiriQueryNotesContext,
        args: ChigiriQueryNotesArgs
    ): ChigiriQueryNotesResult
        return ctx.models.Note:find({})
    end,
}
```

The exact signature is generated from the schema. Rust validates authority,
arguments, stored values, and return values even if a script weakens a Luau
type with `any` or a cast.

## Configuration

`mock.luau` is evaluated once per command in a restricted configuration VM. A
minimal configuration resembles:

```luau
--!strict
local config: ChigiriMockProjectConfig = {
    format = "chigiri-mock-project/v10",
    schema = {sources = {"schema.graphql"}},
    storage = {sources = {"storage.graphql"}},
    clock = {mode = "system"},
    server = {
        listen = "127.0.0.1:0",
        allowed_origins = {"http://localhost:5173"},
    },
    hooks = {
        request = {before = false, after = false},
        operation = {before = false, after = false},
        field = {before = false, after = false},
    },
    state = {fields = {}, initial = {}},
}
return config
```

Public `schema.sources` paths are acquisition locators resolved relative to the
project root, or explicit absolute paths. They are opened by `init`, `update`,
`update --check`, and explicit `reference --schema`, not by internal project
commands. Private `storage.sources` paths must be explicit portable relative
paths confined to the retained project directory; traversal, symlinks, aliases,
and duplicates are rejected. `server.listen` accepts only numeric loopback
addresses. Port zero requests an ephemeral port, which is reported on stderr.
Allowed origins are exact HTTP origins without paths.

A fixed startup clock uses an exact Unix timestamp in seconds:

```luau
clock = {mode = "fixed", now = 1789894800}
```

The startup clock declaration is immutable for the lifetime of one server.
Portable export records that declaration, never the live world policy or a
request-local override.

## Seeds, private storage, and references

Each non-root public model and private storage model has a JSON Lines seed.
An empty file means no records. Every non-empty line is one complete JSON
object. Blank lines, comments, BOMs, duplicate keys, and ill-typed values are
rejected.

```jsonl
{"id":"User-1","name":"Alice"}
{"id":"User-2","name":"Bob"}
```

Runtime `create` may synthesize an omitted eligible ID. Seeds must provide
required IDs explicitly so fixtures remain deterministic.

Private storage SDL may extend a public model with private fields or define a
private-only model. Those values are available to Mock CRUD and relation logic
but never enter public GraphQL validation, introspection, or output.

An object stored inside another record is a fixed snapshot. For example, an
embedded `Image` can keep private ownership metadata for a nested virtual
resolver without exposing it publicly. An independently changing `Wallet`
should use an explicit relation or lookup. Viewer-dependent output belongs in
a virtual field. These are distinct modeling choices: stable embedded values
do not need to become virtual merely because they contain private metadata.

A stored value may refer to one declared key:

```json
{"$ref":{"model":"User","key":{"id":"User-1"}}}
```

Forward and composite-key references are supported. Missing, ambiguous,
cyclic, ill-typed, or excessive expansion fails before the store is installed.
A matching object nested inside a JSON scalar remains literal JSON. Portable
export writes resolved values and contains no `$ref` markers.
The resolved object remains a snapshot; `$ref` does not create a live
relationship.

### Output records versus handmade DTOs

The output boundary distinguishes three origins:

| Origin | Private slots | Behavior |
| --- | --- | --- |
| Genuine unchanged model record | Retained internally | May be wrapped in payloads or lists without losing identity. |
| Validated embedded value read from storage | Retained for nested parents | Remains a stored snapshot, not a live relation. |
| Arbitrary resolver-created DTO | Not allowed | Must match an allowed public output shape. |

A matching ID, copied fields, or a Luau type assertion does not grant record
provenance. Static check validates types, not every runtime value's origin.
If execution reports `MOCK_OUTPUT_PRIVATE_FIELD`, return the genuine model
record unchanged, or build an allowed public-only DTO. Do not change fields on
a record or strip private fields to make a forged record appear authoritative.
For live wallet state, use a declared relation or virtual lookup returning the
stored wallet record. See `chigiri mock reference runtime storage` for examples.

## Standard time and timeleap

Live resolvers and hooks expose bounded standard `os.time`, `os.date`,
`os.difftime`, and `os.clock`. `os.time()` and `os.date` with an omitted or nil
timestamp observe apparent current time. Explicit conversions, `os.difftime`,
`os.clock`, host deadlines, cancellation, logging, and ID generation do not.

Configuration and module initialization allow explicit-input conversions but
reject current-time reads, `os.clock`, and timeleap effects.

For a Boolean resolver with `deadline: String!`, this body compares timestamps
without a type assertion. Both strings must use exactly
`YYYY-MM-DDTHH:MM:SSZ`, with no fractional seconds; the caller must establish
that precondition. Arbitrary RFC 3339 offsets or fractional precision do not
support this lexical ordering rule.

```luau
local nowText = os.date("!%Y-%m-%dT%H:%M:%SZ", assert(os.time()))
if type(nowText) ~= "string" then
    return error("failed to format UTC time")
end
return args.deadline > nowText
```

`os.date` also supports date-table results and may return nil. The pinned
checker can retain that union after a negative type guard ending in plain
`error(...)`; `return error(...)` makes termination explicit. A helper returning
`string` from a positive type branch is another supported form. Numeric epoch
storage is an optional modeling choice, not a requirement for type checking.
The same comparison example is available in `reference runtime stdlib`.

The [guard guidance tests](../crates/mock/src/luau/tests_authoring_guidance.rs)
retain small direct `luaur::check` / `check_with_definitions` reproductions,
separate from Chigiri's checker transformations. When evaluating a future
unmodified public dependency release, rerun these probes and the published
examples before retiring a limitation. They do not authorize dependency
patches, a new module loader, or additional flow rewriting.

Request-local control is allowed only during `request.before` or
`operation.before`:

```luau
local target = os.time({
    year = 2026,
    month = 9,
    day = 20,
    hour = 9,
    min = 0,
    sec = 0,
})
assert(target ~= nil)
timeleap.travel_to(target)
assert(os.time() == target)
timeleap.reset()
```

Omitted options, nil, an empty table, and `{world = false}` select the current
request. Request reset resumes the server policy captured before
`request.before`. Mutation is sealed before field execution, so resolvers,
`field.before`, and all after hooks may read time but cannot move it.

Shared `{world = true}` control is available only through the server-owned
in-process host controller between requests. It is not exposed through
GraphQL, project hooks, HTTP administration, or the CLI. World reset restores
the immutable startup policy without rereading configuration. A world change
affects only request contexts created after publication.

Travel targets must be finite, exactly representable integer Unix seconds from
0 through 9,007,199,254,740,991 inclusive and must pass the native UTC
decomposition and round-trip check on the running platform. `scope`, `global`,
string or fractional timestamps, and `ctx.clock` are rejected.

## Request data, business failure, and atomic execution

`ctx.request` is a readonly transport snapshot, or `ctx.null` outside an HTTP
request. Root and nested resolvers can read it with every hook disabled. A
`request.before` hook and declared state are optional when several resolvers
benefit from shared authentication preparation; they are not prerequisites
for header access. `ctx.fail` publishes an intentional, client-safe business
error; keep tokens and other secrets out of its message:

```luau
ctx.fail({code = "DENIED", message = "The operation is not allowed."})
```

The failure remains sticky even when caught by `pcall`. The complete request
rolls back on rejection, business failure, runtime failure, hard-limit failure,
timeout, or cancellation. A successful request commits only after root
execution and all required after/response stages succeed. HTTP status alone
does not decide whether state commits.

Use a runtime type guard rather than a type assertion to read optional request
metadata. The pinned checker does not narrow `ChigiriHookRequest | ChigiriNull`
from equality with `ctx.null` alone. This Boolean resolver body illustrates a
mock-only demo token, not production authentication:

```luau
local request = ctx.request
if type(request) ~= "table" then
    ctx.fail({code = "UNAUTHENTICATED", message = "An HTTP request is required."})
end
for _, header in request.headers do
    if string.lower(header.name) == "authorization" and header.value == "Bearer local-demo" then
        return true
    end
end
ctx.fail({code = "UNAUTHENTICATED", message = "The demo token is required."})
```

`reference runtime request` supplies this example, nullable duplicate-preserving
`x-tag` values, and optional shared authentication preparation in `request.before`.

Project-local `require` and shared Luau modules are unsupported. There is no
library directory or import option to enable. Keep small helpers local to each
resolver or hook; optional hooks and declared state can share preparation, not
functions. This is a deliberate product boundary, not a missing setup step.

Soft limits emit bounded warning records and allow execution to continue. Hard
limits fail the request. Reference output is the authoritative source for the
current numeric limits and operation contracts.

## Reference and validation

Use scoped reference while authoring:

```sh
"$CHIGIRI" mock reference models --config-dir target/blog-project
"$CHIGIRI" mock reference runtime timeleap --config-dir target/blog-project
"$CHIGIRI" mock reference limits --config-dir target/blog-project
"$CHIGIRI" mock reference all --config-dir target/blog-project --format json
```

The available topics are `directives`, `methods`, `hooks`, `models`, `types`,
`resolvers`, `limits`, `runtime`, and `all`. Runtime selections are `stdlib`,
`request`, `timeleap`, `errors`, and `storage`. Text output uses a fixed
man-style document; JSON uses the closed versioned format. Project reference is
derived from the adopted public contract plus current private SDL and verifies
generated consistency. It does not read external public locators, inspect
resolver bodies or seeds, or claim readiness.

All six hooks support exact item selection, for example
`reference hooks request.before`. Run `reference hooks` to discover the valid
names; guessed `context` or `state` topics are not aliases. Use
`reference runtime` to discover `stdlib`, `request`, `timeleap`, `errors`, and
`storage` before selecting one.

Authoritative model records may be composed into responses unchanged. Persist
changes through model methods; do not assign personalized fields on records or
treat a DTO as a record with selected fields overwritten. Public SDL changes
require schema-owner authorization. After an authorized edit, run canonical
`update --check` when a freshness report is useful, then adopt with `update`;
never patch generated declarations or ownership hashes. If an edit is not
authorized, report the boundary instead of adding a mock-side public field
override.

Run a scoped check after a small edit and full check before serving or export:

```sh
"$CHIGIRI" mock check \
  --config-dir target/blog-project \
  --target resolvers/query/notes.luau \
  --format json
"$CHIGIRI" mock check --config-dir target/blog-project
```

There is no completion inventory. Full validation checks required active files,
closed descriptors, syntax/types, bounded initialization, seeds, identities,
and resource constraints without invoking field/source initializers or hook
`run` bodies. Untouched
intentional-error bodies therefore pass validation and fail only when invoked.

Use `--format json` for the closed `chigiri-mock-check/v3` diagnostic contract.
Diagnostics use logical coordinates and fixed safe vocabulary; they exclude
source values, secrets, and physical paths. Each diagnostic includes up to four
sorted selections from the existing named reference registry. Follow a
selection with the same input mode, for example:

```sh
"$CHIGIRI" mock reference resolvers Query.notes \
  --config-dir target/blog-project
```

Text diagnostics print equivalent targeted commands with safe path
placeholders. An empty `references` list means that no relevant named selection
was proven; it is not an instruction to dump the whole catalog.
Target validation, full readiness, and selected-field HTTP execution prove
different things. A green full check does not establish business semantics or
exercise every nested output selection.

## Public adoption, local code generation, and recovery

After changing private storage SDL or generation-bound settings, regenerate
from the adopted public contract without opening public source paths:

```sh
"$CHIGIRI" mock codegen --config-dir target/blog-project
"$CHIGIRI" mock check --config-dir target/blog-project
```

When an authorized upstream public change is available, compare its normalized
SDL with the adopted contract without writing or checking project readiness:

```sh
"$CHIGIRI" mock update --config-dir target/blog-project --check
"$CHIGIRI" mock update --config-dir target/blog-project \
  --check --format json
```

The comparison exits 0 for `in_sync`, 1 for `update_required`, and 2 for
`unavailable`. If the change should become authoritative, `update` adopts it
and generates against current private inputs in one protected transaction:

```sh
"$CHIGIRI" mock update --config-dir target/blog-project
"$CHIGIRI" mock check --config-dir target/blog-project
```

`update` and `codegen` share the same candidate planner, edit protection, and
transaction/recovery implementation. Neither guesses renames nor silently
rewrites user-owned content. Conflicting generated changes are written as
`.dist` material for explicit reconciliation. Existing resolvers and seeds may
remain as migration input even when they no longer pass the new contract. A
newly added resolver receives the same intentional-error body and does not by
itself prevent full check, export, or restart. Repeating either writer with
identical inputs does not churn generation bytes.

If an update is interrupted, preserve the project and recovery material. Do
not delete snapshots or `.dist` files. Re-run the command after resolving the
reported logical conflict. See
Mock project workflow validation for the
tested transaction and recovery cases.

## Portable export

Export only after full check succeeds:

```sh
"$CHIGIRI" mock export \
  --config-dir target/blog-project \
  --output target/blog-portable.toml
"$CHIGIRI" mock check --config target/blog-portable.toml
"$CHIGIRI" mock serve --config target/blog-portable.toml
```

The portable bundle contains the declared project inputs needed for startup.
It excludes live in-memory records, request-local state, live timeleap policy,
recovery files, and `.dist` material. Export is deterministic for unchanged
project inputs.

## Fresh-destination migration

Every project reader, including `mock update`, rejects old project formats.
ADR-0057 retires the former protected v8 update path. Existing validation of
that historical migration remains evidence only for its original executable.
There is no portable-bundle compatibility loader.

For older, mixed or unverifiable projects/bundles, do not edit only version
strings or copy generated metadata. Preserve the old project as source material
and initialize a separate, absent destination from owned public SDL:

```sh
"$CHIGIRI" mock init \
  --schema path/to/chosen-schema.graphql \
  --config-dir target/blog-project-v10
```

Reapply private-storage declarations and configuration choices deliberately,
then generate fresh owned artifacts:

```sh
"$CHIGIRI" mock codegen --config-dir target/blog-project-v10
```

Only after generation, copy or adapt user-owned resolver function bodies into
the new one-field resolver wrappers, and do the same for hook `run` bodies.
Copy seed records only after reviewing them against the chosen schema. Retain
request-state field declarations, initial values, and hook enablement where
intended. Do not copy `generated/`, `.dist`, transaction, recovery, or other
ownership material, and do not convert the old project in place. Then run:

```sh
"$CHIGIRI" mock check --config-dir target/blog-project-v10
"$CHIGIRI" mock export \
  --config-dir target/blog-project-v10 \
  --output target/blog-project-v11.toml
```

This is explicit source adaptation into a new project. Preserve the original
directory and its ownership/recovery material unchanged.

## Explicit subscriptions and post-commit publication

A generated subscription setup module returns its field's opaque host source,
for example `ctx.pubsub.subscribe.Item({topic = args.topic})` for the
`changed: Item!` field. Public selectors use SDL type names; each field has a distinct generated source and setup-context
type. Only the source returned by setup registers; unused handles do not
activate. It does not return a
Lua closure/coroutine or suspend a VM. The host registers the source at its
linearization point; publications before registration are not replayed.

Mutation resolvers and registered consumer execution have generated
type-specific publish authority, for example `ctx.pubsub.publish.Item({topic = "events",
payload = row})`. The payload must satisfy at least one Subscription destination's complete
shape and Record provenance rules. One capture is delivered once to each
compatible destination of that named type. Query, existing hooks, GraphQL
event resolution and source setup remain read-only and cannot publish. Invalid/caught publication
and hard exhaustion remain sticky request failures.

All roots, hooks and output/encoding work belong to one candidate. Publications
become visible only after the complete Mutation commits; pre-commit failure or
cancellation rolls everything back. Post-commit delivery loss never undoes a
committed store change. Fan-out and clients do not run inside the store exchange.
Captured root values are immutable. Declared relations selected from sealed
captured Records read each event's fresh committed snapshot; arbitrary DTO IDs
cannot gain that authority.

Each subscriber event has a fresh phase context, initial state, hooks, budgets
and a read-only store/clock view, constructed from the retained original bounded
subscriber metadata. Current event policy revalidates that original material;
Chigiri does not refresh credential bytes, and changed credentials require
resubscription. Publisher headers, state patches and local time overrides are
not inherited. Servers do not share topics/state;
restart closes sources and restores configured seeds. The same Mock listener
supports WebSocket, distinct-connection SSE and Apollo multipart; it adds no
broker, replay, receipt protocol, automatic model notifications or Luau network API.

Default delivery uses bounded non-dropping backpressure. A server-authored
source may explicitly select `overflow = "drop_oldest"`; this is not a client
overflow argument. Each source owns a bounded waiting queue before execution,
separate from encoded results after GraphQL execution. Source count, byte and
age limits compose by OR. Default source overflow stops only the affected
source; DropOldest evicts its oldest wholly unprocessed inputs. Encoded
DropOldest evicts only whole unsent event results. The combined actual loss is
reported in `extensions.chigiri.delivery.droppedTotal`, never in public
SDL. See the complete executable [walkthroughs](subscriptions-walkthrough.md)
for exact schema/module signatures, current three-profile consumers, advanced
Apollo metadata handling and cleanup/proxy limitations.

## Timed sources and named consumers

For an adopted field `Subscription.changed: Item!`, setup can return either
`ctx.pubsub.subscribe.Item({topic = "items"})` or a timed source:

```luau
return ctx.generator.sequence.Item({
    values = ctx.models.Item:find({}),
    interval_ms = 1000,
    loop = true,
})
```

This example requires at least one seeded Item. `values` is a dense nonempty
array of at most 1,000 complete field outputs; a list-valued field needs a list
per element, and a nullable field can use `ctx.null`. Interval is an integer
from 1 through 86,400,000 ms. The dataset is captured at setup. Each source
starts after one interval and advances on an independent monotonic cadence,
regardless of demand or apparent time. Late wakes skip positions without a
catch-up burst; skipped ticks are not drops. A finite source emits its final
value once and lets accepted inputs finish. Sequences do not publish implicitly.

Register background consumers explicitly in `mock.luau` and run codegen:

```luau
events = {
    consumers = {
        item_clock = {field = "changed", enabled = true},
        item_audit = {field = "changed", enabled = false},
    },
},
```

```sh
chigiri mock codegen --config-dir .
chigiri mock reference runtime pubsub --config-dir .
chigiri mock reference runtime generator --config-dir .
chigiri mock reference consumers item_clock --config-dir .
```

Codegen creates absent editable `consumers/item_clock.luau` and
`consumers/item_audit.luau` modules and preserves authored bodies. Each returns
exactly `subscribe` and `consume`. The generated signatures for this example are:

```luau
--!strict
local consumer: ChigiriSubscriptionChangedConsumer = {
    subscribe = function(ctx: ChigiriSubscriptionChangedConsumerSetupContext): ChigiriSubscriptionChangedSource
        return ctx.generator.sequence.Item({
            values = ctx.models.Item:find({}),
            interval_ms = 1000,
            loop = true,
        })
    end,
    consume = function(ctx: ChigiriSubscriptionChangedConsumerContext, event: ChigiriSubscriptionChangedEvent): ()
        ctx.pubsub.publish.Item({topic = "items", payload = event})
        ctx.log("info", "Item published", {id = event.id})
    end,
}
return consumer
```

Use reference for the actual root/field names. Setup has no GraphQL argument
object, even for a field with required arguments. It can read models, log and
select one source. Only named setup can use `{all_topics = true}`. Select
exactly one of `topic` or `all_topics = true`; the latter means all
current/future exact topics of the same named type,
not wildcard patterns or replay. Both selections accept the source overflow
policy. Disabled modules are still checked and referenced, but never execute.
All enabled sources establish before HTTP traffic or their sequence clocks.

Each `consume(ctx, event)` runs in a fresh VM and independent write candidate.
It may update models, log and publish, and returns no values. Model changes and
child publications commit together. Ordinary failure rolls back only that
invocation, then processing continues without retry. The publisher's committed
state is preserved. `ctx.request` is `ctx.null`; publisher credentials, state
patches and local time overrides are not inherited. Initial state, apparent
time and model reads are fresh. Genuine captured Records retain their original
values/provenance when republished, even after a same-ID model row changes.
Copies and type assertions cannot invent that authority.

Read-only `ctx.event` contains `topic`, `depth` and `droppedTotal`. Sequence topic
is null. Original inputs have depth zero and child publication adds one edge;
depths one through three are allowed. A depth-four attempt fails the whole
candidate, even through `pcall`; depth-three model-only work can commit.
DropOldest's counter is the consumer's cumulative actual source evictions at
callback start, initially zero; non-dropping delivery uses null. Graceful stops
new generation/callbacks, finishes only already-running work within its deadline
and releases waiting inputs without draining recursive publication chains.

Optional `events.limits` fields use these inclusive defaults and hard maxima:

| Key | Default | Maximum |
| --- | ---: | ---: |
| `max_queued_events` | 32 | 1,024 |
| `max_queued_bytes` | 256 KiB | 4 MiB |
| `max_queue_age_ms` | 10,000 | 60,000 |
| `max_retained_bytes` | 16 MiB | 64 MiB |
| `max_sequence_bytes` | 1 MiB | 16 MiB |

Waiting count excludes the executing input; byte charges last through actual
queued/executing ownership. Definitions have their own per-source limit and
share the retained-source ceiling. Sequence bytes cannot exceed shared bytes.
These are retained-source guarantees, separate from encoded output and process
RSS. Unknown, nonpositive, noninteger or above-maximum values are rejected.

Full check validates contracts without running callbacks or starting timers.
An intentional-error setup may pass checking and still fail enabled startup.
Export includes every registered module and definition, including disabled
ones; a checked bundle can run detached from the original project and SDL.

## Serve, logs, and GraphiQL

Start a checked project and keep access JSON Lines separate from notices:

```sh
"$CHIGIRI" mock serve \
  --config-dir target/blog-project \
  > target/blog-access.jsonl \
  2> target/blog-notice.log
```

Stdout contains only the closed `chigiri-mock-log/v5` JSON Lines stream. Startup
notices, the bound URL, optional GraphiQL URL, worker notices, and shutdown
messages go to stderr. Log queues and records are bounded; log loss never
changes response or readiness semantics. Treat authorization headers, cookies,
request bodies, and user data as secrets and never copy them into hook log
messages.

Automatic subscription setup/publication/drop/terminal records use safe closed
schemas and real request/registration correlation. They exclude topics,
payloads, credentials and private model data. The terminal drop total is frozen
at logical termination, even while charged physical cleanup continues. Logging
is bounded and lossy and is not a delivery receipt or durable audit/replay log.

GraphiQL is disabled by default. Enable it explicitly:

```sh
"$CHIGIRI" mock serve \
  --config-dir target/blog-project \
  --enable-graphiql
```

The default UI path is `/graphiql` and it sends requests to the existing
`/graphql` endpoint. The flag is operational and is not written to project or
portable configuration. Application-owned production integrations remain
responsible for routing, authentication, CORS, TLS, connection policy, and
shutdown.
