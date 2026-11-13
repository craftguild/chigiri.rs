# Chigiri Mock authoring instructions

This directory is an editable Chigiri Mock project. Chigiri provides the schema,
generated Luau types, model API reference, and deterministic validation. Use
your normal coding tools to edit the project, then let `chigiri mock check`
decide whether the result is valid.

## Normal workflow

1. Read the adopted contract with scoped `chigiri mock reference` commands and
   inspect the existing editable `storage.graphql`.
2. Edit `storage.graphql` when private models or private fields are needed.
3. After changing private SDL or generation-bound settings, run
   `chigiri mock codegen --config-dir .`.
4. Inspect `generated/types.d.luau`, then ask `chigiri mock reference` for only
   the model, resolver, type, or runtime capability you need.
5. Edit the smallest necessary set of seed, resolver, or enabled-hook files.
6. Run a target check for each edited set while the rest of the project is
   invalid.
7. Run a full check, then exercise representative GraphQL HTTP scenarios.

Commands assume this directory is the current working directory and
`chigiri` is available on `PATH`:

```sh
chigiri mock codegen --config-dir .
chigiri mock update --config-dir . --check
chigiri mock reference types Todo --config-dir . --format json
chigiri mock reference models Todo --config-dir . --format json
chigiri mock reference resolvers Query.todos --config-dir . --format json
chigiri mock reference hooks request.before --config-dir . --format json
chigiri mock reference runtime request --config-dir . --format json
chigiri mock reference runtime storage --config-dir . --format json
chigiri mock reference runtime stdlib --config-dir . --format json
chigiri mock reference runtime timeleap --config-dir . --format json
chigiri mock check --config-dir . --target resolvers/query/todos.luau --format json
chigiri mock check --config-dir . --format json
chigiri mock serve --config-dir .
```

## File ownership

You may edit these files when they are relevant to the requested behavior:

- `resolvers/**/*.luau`
- `seeds/*.jsonl`
- `storage.graphql` for private models or private fields on public models
- enabled files under `hooks/**/*.luau`

Edit `mock.luau` only when the user explicitly requests a server,
schema/storage-source, clock, hook-registration, or request-state configuration
change.

Do not edit these files unless the user explicitly asks to change the source
schema or regenerate the project:

- `generated/**`
- public SDL files referenced by `mock.luau`
- `*.dist`, orphan, transaction, or recovery material
- this `AGENTS.md`

Do not invent model methods. Read their exact signatures and constraints from
`chigiri mock reference`. Preserve generated resolver signatures. Generated
resolvers intentionally call `ctx.fail` with `NOT_IMPLEMENTED`; this is ordinary
business behavior, not a completion marker, and may be retained or replaced.
Private storage is excluded from public GraphQL validation and introspection;
confirm it through `reference models`, not `reference types`.

## Values, records, and live data

Stored object fields are snapshots. A stable embedded `Image` may include
private metadata declared in `storage.graphql`; nested virtual resolvers may
read that metadata through `parent` while GraphQL exposes only public fields.
A changing `Wallet` belongs behind an explicit relation or lookup instead of
being copied into a snapshot. Viewer-dependent values belong in `@virtual`
fields. Seed `$ref` values copy one validated snapshot; they do not create a
live relationship.

Return model records unchanged when composing responses. Use model operations
for persistence writes. Do not personalize a response by assigning fields on
an authoritative record or by treating a DTO as a record with a few fields
overwritten. Build a declared DTO for an ordinary output shape, or add an
authorized virtual field when the value is derived.

Arbitrary output DTOs may contain public fields only. Private slots are allowed
internally on genuine model records and validated stored embedded values, not
on handmade DTOs. A copied ID or type assertion does not grant record identity.
Static check cannot prove output provenance: `MOCK_OUTPUT_PRIVATE_FIELD` during
execution means a private field reached an arbitrary DTO. Return an unchanged
model record or construct an allowed public-only DTO. See `reference runtime
storage` for the distinction; retain representative nested-field HTTP checks.

`ctx.request` is already available to root and nested resolvers when hooks are
disabled. Read it directly for request-local behavior. Enable
`request.before` and declare request state only when shared authentication
preparation is useful across several resolvers; hooks are optional, not an
access prerequisite. Discover exact surfaces with `reference runtime request`
and exact hook names with `reference hooks`.

For cast-free request access, use `local request = ctx.request` followed by a
`type(request) == "table"` branch, or reject the opposite branch with `ctx.fail`.
Comparing with `ctx.null` alone does not narrow the pinned checker type.
`reference runtime request` contains copyable resolver and optional hook examples.

Project-local `require` and shared Luau modules are unsupported. There is no
library directory, import path, or module-loading option to discover. Keep small
helpers local to each resolver or hook. Request hooks and declared state can
share request preparation, but are not a general shared-function mechanism.

Private fields, private models, nullable values, seed references, and
resolver `parent` values all follow the combined public-plus-storage contract.
Do not use casts or typed temporary locals as the standard fix for an inferred
query literal; generated contextual types should check inline literals. Normal
annotations remain valid when they express real intent, and nullable values
still require ordinary refinement.

Public SDL is owned outside this project unless the user explicitly authorizes
an edit. Its configured paths are acquisition locators; project reference,
validation, serving, and export use the adopted contract. Never invent a
mock-only public-field override. After an authorized public change, use
`chigiri mock update --config-dir . --check`, then adopt it with `update`.
After a private SDL or generation-bound configuration change, use `codegen`.
Do not patch generated declarations, snapshots, or ownership hashes.

Use `os.time`, `os.date`, `os.difftime`, and `os.clock` for standard time
operations. `timeleap.travel_to` and `timeleap.reset` control apparent current
time directly. Their default is request-local and they are available only
during request preparation, such as `request.before` and `operation.before`.
Timestamps are exact integer Unix seconds from 0 through
9,007,199,254,740,991 inclusive and must pass native UTC round-trip. Shared
`world = true` control belongs to the host-owned controller for this server
instance; it is not available through GraphQL, project hooks, or the CLI.
There is no `ctx.clock` compatibility API.

`os.date` can return a string, a date table, or nil. Before comparing formatted
strings, use a `type(value) ~= "string"` guard with `return error(...)`, or a
string-returning helper with a positive type branch. Plain `error(...)` after
the negative guard can leave the union type unrefined in the pinned checker.
See `reference runtime stdlib` for a cast-free comparison example. Lexical
ordering requires matching canonical UTC format and fractional precision;
it is not general RFC 3339 comparison. Extra epoch fields are not required
merely to satisfy type checking.

## Validation meaning

Each diagnostic's `references` list contains at most four exact selections
from the current reference registry. Run the matching targeted command, such
as `chigiri mock reference resolvers Query.todos --config-dir .`, instead of
requesting the whole catalog. Text diagnostics print equivalent commands.

A successful `check --target` proves that the selected current files satisfy
their schema-derived static contracts. It does not mean the whole project can
start. Only a successful check without `--target` reports
`readiness_checked=true` and authorizes `serve`. Validation loads each closed
module table without invoking its `resolve` or `run` function, so it does not
classify or certify implementation progress.

Static validation does not establish business correctness. Review the diff and
exercise representative GraphQL operations, including the selected nested
fields, after the full check succeeds. Target checking, full readiness, and an
actual HTTP scenario prove three different things.
