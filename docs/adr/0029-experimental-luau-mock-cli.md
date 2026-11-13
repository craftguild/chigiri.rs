# ADR-0029: Verify execution, state, and relation boundaries of Luau GraphQL mock CLI with blog fixture

- Status: Accepted
- Implementation: Implemented
- Validation: Gates M-A through M-D, MOCK-01 through MOCK-16, and the initial 64-command common validation passed on macOS.
  The medium-severity debug SIGABRT caused by recursive `pcall` was corrected, and three debug/release regression cases passed.
  Adding the release regression increased the ADR-0029 common validation to 65 commands. The validation record below preserves the final-source evidence for subsequent corrections.
  Independent reviews A and B reported no unresolved findings. Linux and remote CI, and final designer approval, remain distinct from local implementation completion.
  Mock retains its explicit Rust 1.95 exception in package and validation workflows, while the existing product and generated SDKs retain MSRV 1.85.
  See All MOCK-ID traces and observation results.
- Date: 2026-09-08
- Decision owners: Pythia maintainers
- Related: ADR-0004, ADR-0026~0028
- Generated ABI target: Keep 6
- Handoff: [Implementation order](../implementation-handoff.md#17-experimental-luau-mock-cli)
- Implementation goal: Prompt for new agents

## 1. Context and purpose

The long-term goal is to derive Mock behavior from a GraphQL schema for
frontend development through a standalone Rust CLI. AI is limited to initial
configuration authoring; ordinary Rust and scripts own service-time execution
and state. Implementing arbitrary-schema semantic inference, an AI model, and a
custom VM at once would make the execution boundary too large to evaluate.

This ADR therefore runs a fixed blog fixture and measures the integration of
Luau embedded in TOML, Rust's model API, and the GraphQL executor.
The experiment expands the earlier User-only proposal so that creating, updating, and deleting Posts must preserve their relationship to User.
The configuration is not purely declarative: it contains editable resolver scripts. Type checking alone does not prove a script's business correctness.

Passing this experiment does not establish support for arbitrary schema-to-IR-to-Luau conversion, AI generation quality, or a production-ready sandbox.
With the designer's approval, the first version omits external schema input and focuses on a fixed fixture that defines User, Post, and their operations independently.

## 2. Decisions and working boundaries

- Add private package `pythia_mock` and separate binary `pythia-mock` in Lab01. Do not make it a subcommand of the existing `cargo-pythia`.
- `init` creates TOML with a static template, and `serve` reads that TOML. AI, external Lua executable, and Cargo at startup are not required.
- The SDL of the fixture is generated into the ABI 6 contract crate using the existing official CLI. Place handwritten Types/Resolver outside the generation crate.
- Use `pythia::Endpoint` and existing Axum bridge. Do not create new GraphQL parser/executor, HTTP codec, or dynamic schema builder.
- The experimental CLI application owns listening, routing, CORS, admission, workers, and shutdown.
  ADR-0004's rule against adding server-lifecycle ownership to a library or bridge remains unchanged.
- Confine the Luau dependency to the experimental package. Core retains zero regular dependencies, regular consumer dependencies remain unchanged, and production resolver integrity is preserved.
- external applications, the existing generator's public API, ABI, and existing SDL/SDK will not be changed for this experiment.
- User update/deletion, arbitrary models, multiple authors, general relation settings, persistence, authentication, subscription, and AI inference are not covered.
- This is an experiment before the actual release, and backward compatibility of mock settings/API is not guaranteed. There is no old mode or fallback.

Place the handwritten experiment in `crates/mock/` and its fixed SDL/TOML data in that package's fixture directory.
Place the regular generated product in `generated/mock-blog/`, outside the workspace members. Manage its generation manifest from the workspace root and register it with the existing generated-manifest registry, exact checks, and standalone quality checks.
`mock.toml` is the execution setting, and `pythia/config-v2` for generation is the SDK generation setting during development, so they are not read as the same format.

### 2.1 2026-09-08 amendment: MSRV for mock package only

During WP-M0, luaur 0.1.8 failed to compile with Rust 1.85.0, while its VM unit test passed with Rust 1.95.0.
With explicit approval from the decision owner, only `pythia_mock` sets `rust-version = "1.95"`.
This experimental choice records the lowest compatibility level measured for this package; it does not claim that Rust 1.95 is luaur's intrinsic minimum version.

- `pythia_mock` is an explicit member of the same workspace, and the rust-version is specified in the package itself.
  Do not change workspace.package 1.85 and root default toolchain 1.85.0.
- `default-members` remains limited to existing regular packages, so bare Cargo commands do not implicitly include Mock.
  Run Mock explicitly with `cargo +1.95.0 ... -p pythia_mock`, and select the whole workspace with the corresponding toolchain's `--workspace` option.
- Keep framework/runtime, `cargo-pythia`, and generated crate (including mock-blog) compatible with 1.85.
  ABI 6, generated Cargo/marker rust-version, generated formatter fixed at 1.85.0 will not be changed.
- Mock test/Clippy/rustdoc/build requires 1.95.0, and normal source formatting also uses a compatible toolchain.
  Don't require the Rust compiler for users who only use distributed mock binaries.
- Exceptions are limited to mocks and their VM verification packages. Do not break existing consumer 1.85 compatibility by changing shared dependencies/lockfile.
  Compatibility patches from third-party sources, changes to another VM, and increases in MSRV of regular products are not included in this approval.

Divide validation according to the matrix in Section 8. Do not repeatedly demonstrate the known Rust 1.85 Mock compilation failure or use publishing or a pending commit as a restart condition. Dependency, VM, state-management, and other safety requirements remain unchanged.

## 3. Blog model contract

### 3.1 GraphQL surface

The following fixed SDL is the source of truth. Generate its Rust type contract with the existing CLI and test it against the fixed Luau type declarations.
```graphql
type User {
  id: ID!
  name: String!
  posts(limit: Int! = 20): [Post!]!
}

type Post {
  id: ID!
  title: String!
  body: String!
  author: User!
}

input CreateUserInput {
  name: String!
}

input CreatePostInput {
  title: String!
  body: String!
  authorId: ID!
}

input UpdatePostInput {
  title: String
  body: String
  authorId: ID
}

type Query {
  getUsers(limit: Int! = 20): [User!]!
  findPost(id: ID, authorId: ID, limit: Int! = 20): [Post!]!
}

type Mutation {
  createUser(input: CreateUserInput!): User!
  createPost(input: CreatePostInput!): Post!
  updatePost(id: ID!, input: UpdatePostInput!): Post!
  deletePost(id: ID!): Boolean!
}
```
### 3.2 Saved values and relations

- Save `id/name` for User and `id/title/body/authorId` for Post. The ID should be a non-empty string that is unique for each model.
- Post refers to one User who always exists, and User has 0 or more Posts. Not many-to-many representing co-authors.
- The only positive relation is `Post.authorId`. Do not create a separate storage array for `User.posts` and reverse lookup from Post.
- The seed checks Post references after validating all Users. Reject duplicate IDs and expired references before listening.
- Rust determines ID numbers to avoid conflicts with seeds. Do not change the ID when updating, and include the numbering status in the transaction.
  The counter for each model advances the candidates from 1, and the seed ID is reserved even after deletion. The new ID counter is returned only when rolling back.
- Maintain registration order. Update and author change do not change the order, and deletion removes only the target.
- Do not save relations as Lua circular tables or GraphQL response JSON.

### 3.3 Meaning of operations

| Operation | Contract |
| --- | --- |
| getUsers | Return Users in registration order, subject to the limit. |
| findPost | Always return an array. Combine `id` and `authorId` with AND; omitted or null values do not filter. Apply the limit after filtering. |
| createUser | Create a User from its name and return it with the generated ID. |
| createPost | Create a Post from `title`, `body`, and `authorId`; reject an unknown author. |
| updatePost | Change only specified fields. Preserve omitted fields, reject explicit null, and return the unchanged Post for an empty input. |
| deletePost | Return true when deletion succeeds and false when the Post is absent. Never delete any other Post. |

Even when an ID is specified, `findPost` returns an array containing zero or one item; do not add `findOne` or `findAll` variants.
Updating a nonexistent Post is an error, not an upsert. Changing the author also requires the referenced User to exist.
Nullable fields in `UpdatePostInput` represent omission; they do not permit required model fields to become null.
There is no business prohibition for empty name/title/body, and only the byte limit in Section 7 applies.

## 4. CLI and editable TOML

```sh
pythia-mock --help
pythia-mock --version
pythia-mock init --out mock.toml
pythia-mock serve --config mock.toml
```
- `--out` and `--config` are required explicitly. There is no implicit search,
  configuration override through CLI flags or environment variables, or
  tolerance for unknown options.
- `init` exclusively creates an absent file whose parent directory already
  exists. It never overwrites an existing file, directory, or link.
- `init` does not generate a schema or SDK, and `serve` does not regenerate or
  rewrite the file. The TOML is an artifact edited by the user.
- `help` and `version` work without configuration or network access. `version`
  reports the binary package and embedded VM versions without consulting the
  current directory's Git state.
- Root `format`, `fixture`, `server`, `seed`, and `resolvers` entries are
  required. Every level is closed and rejects unknown or duplicate keys.
  `seed.users` and `seed.posts` may be empty arrays but may not be omitted. The
  root resolver table must contain exactly the following six entries.
- `serve` validates the TOML, seed, and every script's syntax, types, and
  compilation before binding. There is no hot reload; restart after editing.
- Exit status 0 means successful completion, 1 means input or validation
  failure, and 2 means CLI usage, environment, or worker failure.

The following is the fixed output from `init`. Repeated initialization with the
same binary is byte-identical and contains neither timestamps nor randomness.
```toml
format = "pythia-mock/v0"
fixture = "blog-v1"

[server]
listen = "127.0.0.1:4000"
allowed_origins = ["http://localhost:5173"]

[[seed.users]]
id = "user-1"
name = "Alice"

[[seed.users]]
id = "user-2"
name = "Bob"

[[seed.posts]]
id = "post-1"
author_id = "user-1"
title = "First post"
body = "Post body"

[resolvers."Query.getUsers"]
luau = '''
return ctx.models.User:find({ limit = args.limit })
'''

[resolvers."Query.findPost"]
luau = '''
return ctx.models.Post:find({
    id = args.id,
    authorId = args.authorId,
    limit = args.limit,
})
'''

[resolvers."Mutation.createUser"]
luau = '''
return ctx.models.User:create(args.input)
'''

[resolvers."Mutation.createPost"]
luau = '''
return ctx.models.Post:create(args.input)
'''

[resolvers."Mutation.updatePost"]
luau = '''
return ctx.models.Post:update(args.id, args.input)
'''

[resolvers."Mutation.deletePost"]
luau = '''
return ctx.models.Post:delete(args.id)
'''
```
Seed's `author_id` and GraphQL/Luau's `authorId` are explicitly converted on the Rust side. No alias is provided that allows both notations.
The script is the body of the resolver function and does not generate the entire HTTP response or any entrypoint function.

## 5. Model API and types to inject into Luau

Use `luaur 0.1.8` as the initial candidate and pin the selected dependency in
the lockfile. Enable library use only; do not require an external Lua or Roblox
runtime, JIT, REPL, or AI-model download. As Section 2.1 specifies, Mock's MSRV
is 1.95 while existing products remain on 1.85. An API being present does not
prove that it behaves correctly, so the first focused test measures type
checking, host calls, interruption, and recovery.

- `QueryContext` exposes only `find` for User and Post. `MutationContext` adds
  `User.create` and `Post.create`, `update`, and `delete`.
- Derive the Luau User/Post records, inputs, arguments, contexts, and expected
  return values from the fixed model contract.
  Post records have `authorId` and do not recursively embed GraphQL `author` or dereferences.
- Insert the TOML body into a `--!strict` typed resolver wrapper and validate it
  with the host type declarations. Check it against the expected argument and
  return types; successful inference of the script in isolation is insufficient.
- Resolve GraphQL `Input::Undefined`, `Null`, and `Value` before calling Luau.
  Rust rejects an explicit null in an update; only an omitted field becomes
  nil/unset. An omitted or null filter is normalized to no filtering.
- `find`, `create`, and `update` return copies of model records; `delete`
  returns a Boolean. Mutating a returned table does not mutate the store. Root
  results are checked for type, ID, and references and must match the stored
  record in the read snapshot. Reject fabricated IDs and modified copies that
  do not match storage. General-purpose computed fields are out of scope.
- The Rust host also validates operation authority, argument types, known
  fields, integer ranges, capacity, and references. Luau casts or other static
  checking evasions cannot turn a Query into a write, and scripts cannot
  override a model field's type or limit.
- Represent IDs as Luau strings and limits as range-checked integers. Do not
  truncate floating-point values or implicitly convert strings to numbers.

Syntax, type, or compilation failures are returned before listening. Runtime
errors use the existing GraphQL field-error path.
The initial environment exposes `assert/error/type/pairs/ipairs`, root-specific `_G`, and `pcall` that Rust monitors.
`pcall` receives a function and its arguments, and returns true and a return value on success, and false and a fixed diagnostic string on failure.
Memory exhaustion is recorded as a sticky failure at this boundary; catching
an error with ordinary `pcall` cannot erase a budget failure. Re-entry from a
Rust callback into Luau through concurrent `pcall` calls is limited to 16
levels. Level 17 fails before `Function.call`. This protects the native stack
and is separate from the VM's time and memory budgets. Decrement the depth on
both success and ordinary-error paths; it is a nesting depth, not a cumulative
call count. Re-entry overflow is also sticky: even if the script catches it and
returns normally, the root fails and a Mutation does not commit.
The OS/library set and dynamic load will not be exposed, and existing metatables of string type will also be removed.
Do not create a strong reference cycle for the VM by capturing the Lua handle to the host closure.
Diagnosis shows the basename of config, resolver coordinate, TOML position, line/column of Luau main body, and failure stage.
Do not report only line numbers inside the wrapper. Do not transfer the entire script, request value, or absolute path to HTTP responses or standard diagnostics.
Static type checking is not claimed to prevent Luau `any`/cast misuse or host
implementation errors.

## 6. GraphQL, workers, transactions, relations

The CLI uses the existing Endpoint/bridge and passes the coercioned owned input to the dedicated worker.
The worker owns the Luau VM and store, and does not `Send` the Lua handle to the resolver future.
The initial version processes GraphQL operations one by one. This is the application's admission,
It does not change the Query field parallel poll or Mutation root serial order of existing executors.

Each root invocation executes in a new VM environment and does not carry over script global/closure as state to the next invocation.
Compiled bytecode may be reused, but request handling does not repeat type checking. Rust's store is the only persistent state destination.
The persistent destination here refers only to the process, and is not saved to a file or DB.

Mutation commit steps:

1. Create a working copy that includes both User/Post stores, registration order, and numbering status.
2. Apply all host writes to the copy, allowing subsequent finds within the same script to read the changes.
3. Do not change the live store until the script completes successfully, the return value is converted to Rust, and all models/references/capacity checks are successful.
4. Check for cancellation/expiration just before commit, and replace the entire store only if successful. This exchange is the commit point.
5. Discard the entire working copy on failure; never apply even a small subset incrementally to live state.

If Post.create is performed following User.create within one Mutation script, it is considered one transaction.
If host inspection fails or budget is exceeded, it will be treated as a transaction failure and will not be committed even if the script catches an exception.
Do not combine a request containing multiple root mutations into one transaction. Failure of a later root must not roll back an earlier root that already committed.
HTTP disconnection after commit, response encode failure, and nested completion error do not rollback.
If it has already been committed due to a conflict between cancel and commit, do not pretend that it has not been updated. Exactly-once guarantees until response receipt are not covered.

Connect the read snapshot to the query result and the mutation result after commit, and the User/Post object resolver reads the value from there.
`Post.author` reversely looks up User from authorId, and `User.posts` reversely looks up Post from the same snapshot.
Do not switch to a new live state during nested traversal of the same root result, and do not mix old User and new Post.
Relations resolve only selected fields. Do not confuse circular type references with a finite selection or pre-expand the entire graph.
Apply the same limit/registration order as findPost to `User.posts(limit)` and do not return deleted Posts.

## 7. Resources and operational boundaries for small experiments| Target | Initial limit/default |
| --- | --- |
| TOML | 256 KiB, UTF-8, bounded read of regular file |
| Each Luau script | 32 KiB, limited separately for main body and wrapper |
| User / Post | 1,000 each. ID is non-empty and less than or equal to 256 UTF-8 bytes |
| name / title / body | name/title each 256 UTF-8 bytes, body 4 KiB |
| find / User.posts | limit is 0 to 100, default 20. Reject negative/excess values without clamping |
| VM for each call | 16 MiB, execution budget 500 ms, up to 256 host API calls |
| Simultaneous pcall reentrant of Rust↔Luau | Up to 16 stages. The 17th stage rejects before re-entry and maintains root failure even after capture |
| worker queue | 16 results. Do not spawn synchronous block/unlimited tasks when full |
| operation / shutdown | operation 5 seconds, common deadline 5 seconds from shutdown start |

These are fixed values for experimental purposes, and no general-purpose policy language or upper limit removal option is added to TOML.
Maintain the body/output-node/response-byte/selection-depth restrictions of the existing Endpoint and do not select unbounded.
Do not create an unlimited request queue before admission, and reject as busy if there is no operation permit.

Luau's standard environment is limited to the minimum necessary and does not expose filesystem/network/process, require, or dynamic code load.
Passing script verification and granting a host capability are separate decisions. Do not infer the available standard library from API names alone; test the globals that are actually published.
Keep scanning, allocation, and conversion within their limits, including work performed by Rust callbacks.
The VM memory limit is not the overall limit for Rust's store, compiler/typechecker, and process RSS.

VM interrupts stop execution when a budget is exceeded. The VM and working
copy are discarded, and the server remains available for the next valid
request. A Tokio timeout or dropped `JoinHandle` alone does not count as
stopping a synchronous VM. Shutdown signals stop new admission, notify active
work of cancellation, and drive worker collection.
If a thread cannot be stopped and recovered safely, do not detach it silently or mark the case Passed. Retain the reproduction material, record the gates that were not reached, and reevaluate the approach.
At this stage, we cannot guarantee the safety of OS sandbox or hostile multi-tenant execution.

Bind is limited to numeric loopback addresses and defaults to 127.0.0.1:4000. Port 0 displays the actual bind destination so that it can be used for testing.
CORS only allows the configured origin, not the credential. Preflight is handled by CLI routing.
The GraphQL/HTTP error format passes through the existing Endpoint and does not fallback script failure to a normal empty array.

## 8. Implementation order and achievement gate

First exercise the VM in isolation, then pass through the normal `init` and
`serve` path, and finally add negative tests for relations, rollback, and
interruption. Separate the fixed-model contract representation from script
candidate creation, but do not build an unused general-purpose IR or AI-provider
framework.

| ID | What to demonstrate |
| --- | --- |
| MOCK-01 | Compile the selected luaur version with Mock MSRV 1.95.0 and stable, distinguish valid and invalid typed wrappers, and execute Rust host calls. |
| MOCK-02 | init deterministic, non-overwriting of existing files, TOML including all 6 scripts, offline help/version |
| MOCK-03 | Before listening, reject unknown, duplicate, or missing settings; invalid script types; duplicate IDs; and dangling seed references. |
| MOCK-04 | Start the real binary on a temporary port and exercise HTTP `getUsers`, `createUser`, and every Post CRUD operation. |
| MOCK-05 | Cover find-by-ID/author, AND, limit, default, and null semantics; zero, one, and multiple results; registration order; and absent update/delete targets. |
| MOCK-06 | With at least two Users, verify both relation directions after Post creation, author replacement, and deletion. |
| MOCK-07 | Cover variables, aliases, fragments, selected-field-only responses, User→posts→author, null/error behavior, and existing GraphQL limits. |
| MOCK-08 | Unknown author create/update, update omitted/null/empty input, ID unchanged, capacity/byte/limit boundary |
| MOCK-09 | After a script error or invalid return from create/update/delete, both stores, registration order, and ID allocation state remain unchanged. |
| MOCK-10 | Create a User and Post in one script; commit both on success and roll back both on an intermediate failure or caught host error. |
| MOCK-11 | Static/dynamic rejection of query write, non-persistence of Lua table modification, rejection of invalid/fictitious records |
| MOCK-12 | Stop infinite loops, excessive reservation, and excessive host calls; then verify a subsequent normal request succeeds and early cancellation does not commit. In debug and release modes, verify the `pcall` reentry limit, rollback of both models, and restoration of ID numbering. |
| MOCK-13 | Verify serial operation admission, snapshot consistency, worker collection on signal, and seed restoration after restart. |
| MOCK-14 | Editing the TOML changes Luau filter/update behavior, and diagnostics include coordinate, body position, and failure stage. |
| MOCK-15 | Cover loopback HTTP, CORS preflight, unknown origins, unavailable VM capabilities, busy admission, and limit rejection. |
| MOCK-16 | Verify generated-output exactness and quality, existing-product regressions, implementation rules, measured results, and untested environments. |

Gate M-A requires MOCK-01 through MOCK-04; M-B requires MOCK-05 through MOCK-11 plus MOCK-14; M-C requires MOCK-12, MOCK-13, and MOCK-15; and M-D requires MOCK-16. Every gate requires a concrete test trace.
The validation record maps each ID to its test name, assertions, common-validation result, and measured values.
The Passed result is based on every gate, including real CLI/HTTP execution,
not only earlier WP-M0 partial measurements or added design documents.
The stop test is executed from a subprocess with an external watchdog, the test runner is not lost due to hang, and rescue by kill is not counted as a successful VM recovery.

Use focused tests and regular cache during development, and run the full product suite all at once after the final source change.
Preserve the 400-line handwritten-file limit, the English prose rule from ADR-0043, semantic generated-file layout, ownership boundaries, and exact checks.
Connect the new experiment tests and generated crate to common validation. Do not add Luau dependencies to regular consumers.
Partition the required `./scripts/validate.sh` checks as follows, and invoke the same entry point from required Linux and macOS CI.

| Toolchain | Verification target |
| --- | --- |
| 1.85.0 | Regular checks for all existing packages, consumers, generators, and generated crates; exclude only Mock packages from the MSRV run |
| 1.95.0 | Run Mock build, test, Clippy, rustdoc, and formatting checks, plus real HTTP coverage for every MOCK-ID. Also run the doctor against Mock as a consumer. |
| stable | Regular checks for the entire workspace and all generated crates and consumers, including Mock |

The mock-blog alone is verified to be compatible with 1.85, and the mock application wiring is verified to be compatible with 1.95.
Actual HTTP regression of pcall reentrancy is mandatory for both debug/release of fixed 1.95.0, and rollback of captured MemoryError is also checked at the same time.
In 1.85, the `--workspace` check specifies the target with `--exclude pythia_mock`, etc., and does not create a mode that ignores failures.
If the spike remains an independent package, register it explicitly in the 1.95.0 and stable execution plans.
CI explicitly installs 1.95.0 and records the executed rustc version. Floating stable only does not replace fixed 1.95.0 validation.
Regression tests ensure aggregation fails on shared dependency-resolution errors, omitted or duplicate validation targets, and failed or omitted Mock checks.
Existing products remain formatted with Rust 1.85.0; do not apply a repository-wide formatter-version change.
For a document-only revision, check the implementation rules, links, example settings, and diffs without claiming new implementation-test evidence.

Measurements include release binary bytes, cold/incremental build time, startup time, normal/limit test RSS, and diagnostic examples.
Use a dedicated target directory for cold measurements without deleting other projects or caches. Record the command, toolchain, host, adopted VM version, and measured target.
For capacity/speed, do not fabricate the passing threshold before measuring, and use the experimental results for subsequent acceptance/rejection decisions.
SSH acquisition, package, clean commit, publish, push, tag, official load/long fuzz are not gates this time.
When completed, clarify untracked artifacts, target CI jobs, unexecuted environments, and update startup examples and verification records.

## 9. Consequences and next steps

By reusing the path to generate a regular Rust contract from SDL, we can limit Luau's dynamic behavior to inside a regular typed resolver.
On the other hand, implementation costs for consistency with Luau type declaration of fixed fixtures, table/Rust conversion, and VM stop/recovery remain and are subject to actual measurement.
Evaluate whether a short script can handle cross-model references and transactions; assess AI accuracy separately from server consistency.

The future replacement point will be the process of creating resolver script candidates from the model contract.
Even if an AI response later replaces the static template, Rust owns expected types, host privileges, store constraints, and inspection and compilation entry points.
Do not create an entry point that lets AI alter the schema, privileges, or limits or run without inspection.
External schema → IR, arbitrary model/relation, AI prompt/model/inference, and custom scalar expressions are designed in separate ADR.
Passing this blog experiment alone neither proves the scripts' business validity nor justifies replacing the production server that handles assets.

## 10. Research basis

When Lab01 was investigated, HEAD was `5b1ea536c98e013b00f2a35e9bcd95d9015f41b0`. The review covered the fixed Rust descriptor in `crates/pythia/src/schema.rs`, erased dispatch in `crates/core/src/executor/value.rs`, and `examples/axum-adoption/src/server.rs`. This historical reference is not an instruction to reset or check out that revision.

The initial `luaur` API research is not an implementation guarantee. Reconfirm the following behavior against the adopted version.

- [luaur](https://github.com/pjankiewicz/luaur): Rust implementation of compiler/VM/typechecker.
- [check_with_definitions](https://docs.rs/luaur/0.1.8/luaur/fn.check_with_definitions.html): host type declaration and structured type diagnostics.
- [Lua API](https://docs.rs/luaur-rt/0.1.8/luaur_rt/state/struct.Lua.html): Application boundaries for interrupt, memory, and stdlib/host calls.
