# ADR-0032: Separate Mock into editing directory and provide edit-protected update and portable export

- Status: Accepted
- Implementation: Implemented (2026-09-09; local MP-01 through MP-32 and Gates MP-A through MP-E passed)
- Designer review: Passed (2026-09-09; Critical / High / Medium / Low = 0 / 0 / 0 / 0; see the verification record)
- Date: 2026-09-08
- Decision owners: Pythia maintainers
- Depends on: Mock IR of ADR-0030, model API of ADR-0031 and reference derived from implementation contract
- Related: ADR-0026 through ADR-0029
- Scope: `pythia-mock` editing project, configuration Luau, typed resolver module, JSONL seed, update/check/export/reference
- Activation: Implemented as a separate task after ADR-0031 was completed and reviewed; it does not retroactively change ADR-0031's completion criteria.
- Implementation goal: Goal prompt for new agents

Planned successor: [ADR-0051](0051-mock-contract-validation-without-completion-state.md)
removes resolver completion `state`, bundle completion metadata, and the
associated admission gate at implementation cutover. Its clean-break versions
and fresh-destination migration supersede the corresponding contracts below.
Typed closed modules, hash-based update, ownership, and recovery remain required;
previous implementation and validation results remain historical evidence.

## 1. Background and decisions

The self-contained TOML from ADR-0030 and ADR-0031 is suitable for distribution, but combining SDL, types, resolvers, and seeds in one file makes large schemas difficult to edit, inspect, and regenerate after schema changes.
This ADR separates "project for editing" and "single artifact for distribution."

1. Use a directory for editing, separate settings into `mock.luau`, and separate resolvers into Luau modules for each GraphQL field.
2. Collect type declarations derived from schema/API into the generated area, and do not embed generated types in the handwritten resolver.
3. `update` replaces an unedited resolver. It preserves an edited resolver and writes the new candidate to `.dist`; the user performs the merge.
4. Seed is JSONL for each model, and check saved fields, constraints, and relationships between all models before listening.
5. Create a self-contained TOML using export only when necessary. Execute directory/bundle using the same IR, executor, and store.

Configuration Luau is not parsed as a special literal-only syntax. It runs in a bounded VM and must produce a closed configuration value.
`reference --config-dir` also only performs configuration. Do not mix resolver or seed inspection/execution with reference.
This ADR includes type-declaration generation and type checking. VS Code extensions, LSP settings, and editor-integration demonstrations remain separate work.

Adding this ADR alone does not mean implementation has started or completed or the new gate has passed.
The code, goal, and acceptance record of ADR-0031 will not be changed midway to accommodate this design addition.

## 2. Format, compatibility, and non-targeting

Because the Mock CLI was not yet public, this change introduces the following formats without a backward-compatibility layer.

| Boundary | Identifier | Meaning |
| --- | --- | --- |
| Edit project | `pythia-mock-project/v1` | Settings Closed configuration and directory layout returned by Luau |
| Single TOML bundle | `pythia-mock/v3` | Distribution artifact including evaluated configuration, SDL, resolver source, and seed |
| model host API | `pythia-mock-api/v2` | Semantics of operations, permissions, and values of ADR-0031. No changes this time |
| reference JSON | `pythia-mock-reference/v2` | Structured reference including project input and whether to run configuration |

The host API version is uniquely determined from the format, and users are not required to select different permissions.
Automatic conversion, alias, and double implementation with old mock TOML, old `init --out`, and old reference JSON will not be left behind.
Existing users explicitly migrate Luau and seeds into the new project. Importing a bundle back into an editable project is out of scope.

The following contracts remain unchanged:

- ABI 6 and MSRV 1.85 for the regular product and generated SDK, Mock-only MSRV 1.95.0, type checking with `luaur = 0.1.8`, and core's regular dependency boundary.
- SDL and Mock IR remain authoritative for types, models, relations, and resolver contracts. Type declarations, snapshots, and references are derived artifacts.
- Rust side permission/type/capacity checks, root unit transactions, snapshots, cancellation/commit judgment, sticky failure.
- Semantics of ADR-0030/0031: Query/nested is read-only, only Mutation root can be written.

AI inference, persistence, Subscription, hot reload, arbitrary module imports, automatic source merging, and dumps of the running store are out of scope. Do not add a production sandbox or publisher, SSH, or package-release gates.

## 3. Editable project and ownership scope

```text
mock/
├── mock.luau
├── generated/
│   ├── types.d.luau
│   ├── schema.snapshot.json
│   └── update-state.json
├── resolvers/
│   ├── query/<field>.luau
│   ├── mutation/<field>.luau
│   └── objects/<Type>/<field>.luau
└── seeds/<Model>.jsonl
```
Query/Mutation is the directory name of the operation role, and does not require the root type name of SDL to be `Query`/`Mutation`.
The nested handwritten resolver is in object/field units. Do not create unnecessary stubs for save fields and automatic relations.
If the same coordinate serves as both root and nested, use one module and check the context type and runtime permissions of both.
File names and public type names are stabilized based on coordinate, and sequential numbers are not reassigned when unrelated fields are added.
Reserved names, case-insensitive filesystems, collisions after normalization are rejected before writing, and different coordinates are not assigned to the same path.

| Area | Ownership/update policy |
| --- | --- |
| `mock.luau` | Owned by the user after init. Do not automatically overwrite with update |
| `generated/` | CLI owned. Validate and replace owned inventory and schema/update generations |
| active resolver | Initial candidates are generated by CLI. Then protect your edits with Section 7 B/U/N comparison |
| `.luau.dist` | CLI migration proposal. Not subject to execution. Also protects changes made by users |
| `seeds/` | User-owned. Create an empty JSONL for the new model exclusively, and do not update the existing content |
| orphan | Old resolver/seed that is no longer part of the schema. Keep but exclude from active inspection/execution/export |

No implicit glob searches or arbitrary Luau file execution. Active inventory is determined from the current IR.
The snapshot is used to explain the difference from the previous version, and is not used as a runtime authority in place of the original SDL.
Existing ownership verification principles are applied to the classification between handwritten and generated, and line counts are not exempted just by self-proclaimed header or directory names.

## 4. Settings Luau/external SDL/resolver module

### 4.1 Bounded execution of configuration

The setting example is as follows. `PythiaMockProjectConfig` is a declaration that the CLI supplies for generation and type checking.
```lua
--!strict
local config: PythiaMockProjectConfig = {
    format = "pythia-mock-project/v1",
    schema = {
        sources = {
            "../backend/graphql/types.graphql",
            "../backend/graphql/operations.graphql",
        },
    },
    server = {
        listen = "127.0.0.1:4000",
        allowed_origins = {},
    },
}
return config
```
Configuration may use ordinary Luau local calculations, functions, and control
flow, but its return value is limited to the closed plain-data shape above.
Reject unknown keys, type mismatches, function/thread/userdata, cycles, tables with metatables, and invalid sparse arrays.
Rather than relying solely on a successful typecheck, the return value is re-examined on the Rust side and converted to an owned configuration.
Multiple assignments to the same table are Luau semantics and should not be confused with JSON's duplicate-key check.
The server will check the existing numeric loopback, CORS, and port 0 contracts, and will not relax to public bind due to Luauization.

A single command evaluates the settings only once and stores the results. Do not re-run the configuration source later.
Do not pass Lua handle or closure to IR/worker after destroying VM.
Do not expose `ctx`, store, filesystem, network, process, environment variables, clock, random numbers, `require`/`load` to the configuration VM.
Retain the minimal global allowlist and string-metatable isolation; do not expose
the complete standard library.
The same protections in Section 10 apply to VM shutdowns, memory, pcall reentries, and caught failures.

`schema.sources` is a non-empty explicit array. Relative path is based on `mock.luau` directory, and absolute path is also allowed as explicit input.
Do not copy the original SDL to the project and make it a second authoritative schema. Reading SDL outside the project is the responsibility of the Rust side.
Duplicate specifications of URLs, globs, and the same file are rejected, and the declaration order and logical source name are preserved.
Each command recreates the IR from the current SDL contents. Physical absolute path should not be included in bundle or normal diagnostic.
init records a reference to the specified SDL. No implicit cwd search/snapshot fallback.

### 4.2 Typed resolver module

The resolver is not just a body fragment, but a Luau module that returns a table with only `state` and `resolve`.
The following is an example of a schema with a `User` model and `getUsers` field, and the type name, arguments, and return value are generated from the actual IR.
```lua
--!strict
local resolver: PythiaQueryGetUsersResolver = {
    state = "ready",
    resolve = function(ctx: PythiaReadContext, args: PythiaQueryGetUsersArgs): {PythiaUserRecord}
        return ctx.models["User"]:find({ limit = 20 })
    end,
}
return resolver
```
Manual scaffold should be `state = "unimplemented"`. Don't silently return an empty array/null to make it look like it's implemented.
If required resolver is not implemented, check/serve/export will be rejected.
Parse/typecheck the module independently and match it with the expected coordinate-specific type. Do not allow escape of string wrapper.
Do not use `any` or cast to bypass Rust's permission, argument, or result checks.
The type name is fixed in semantic units, and the temporary index on the IR is not exposed to the public name written by the user.
The argument order maintains the existing `resolve(ctx, args[, parent])`. typed parent for nested, root and nested
Pass a nullable parent and make the Mutation root and nested context a read/write union.

`types.d.luau` is generated from the current Mock IR and ADR-0031 typed API contract.
Contains the types of context/model/record/input/args/result and configuration/module, and passes the same definition to the CLI typechecker.
check/serve/export detects declaration drift and does not make modified declarations on disk type authority. No implicit updates.
This contract covers type declaration generation and type checking, and does not claim that any editor will complete it just by placing `.d.luau`.

The pre-startup check executes bounded module initialization and checks closed state/functions, but does not call the `resolve` body.
In the actual request, use fresh VM for each resolver as before, and include module initialization and resolve in the same deadline.
Validate the same module contract on every initialization without passing
`ctx`. Globals and closure state are not cross-request storage.
Changes to the runtime store will continue to be made only via the Rust host API.

## 5. CLI success criteria

```text
pythia-mock init --schema SDL [--schema SDL ...] --config-dir DIR
pythia-mock update --config-dir DIR
pythia-mock check (--config-dir DIR | --config FILE)
pythia-mock serve (--config-dir DIR | --config FILE)
pythia-mock reference [--schema SDL ... | --config-dir DIR | --config FILE] [--format text|json] [--no-pager]
pythia-mock export --config-dir DIR --output FILE
```
The parentheses and `|` are for explanation and are not input to the shell. The two inputs serve/check are **required and exclusive**.
If there is no input, reference will display the common API, and if there is input, it will only allow one type of schema/project/bundle.
Make usage errors for duplicate single options, unknown options, and multiple input types. No format inference, implicit search, or setting override.
Keep `--help`/`--version` and existing build identity display.

| Command | What exit status 0 guarantees |
| --- | --- |
| init | A new project is placed; manual resolvers and empty seeds are not claimed to be operationally complete. |
| update | Candidates, types, and management metadata were placed with edit protection. User migration and runtime readiness are evaluated separately. |
| check | The same static, initialization, and seed checks required by `serve` passed, without listening or invoking a resolver body. |
| serve | HTTP started from an owned snapshot that passed every pre-boot check. |
| reference | A reference was constructed from the input; runtime readiness was not checked. |
| export | A new single-file bundle was written from the same snapshot that passed `check`. |

Exit status 0 means the command completed successfully, 1 means a configuration, SDL, type, seed, or ownership inconsistency, and 2 means a CLI-usage, environment, or I/O failure.
`update` returns 0 when it successfully places a `.dist` proposal or a new manual scaffold; that status does not claim runtime readiness.
Its result identifies changed, retained, suggested, and orphaned files, states any required manual action, and sets `readiness_checked = false`.
Projects that have not been successfully checked will not be reported as startable or migrated based on the update results alone.
`init` accepts only a new directory, and `export` accepts only a new file. This version provides no force or overwrite alias.

## 6. JSONL seed and pre-boot inspection

The seed for each model is UTF-8 JSONL, and one record object is written per line.
An empty file contains zero records. LF and CRLF line endings are accepted, and the final newline may be omitted. Blank lines, comments, BOMs, and non-object values are rejected.
Share the existing strict JSON decoder and do not allow duplicate keys, integer rounding, deep recursion, or over-allocating via separate routes.
Rows are read sequentially under aggregate raw-byte, record, and node budgets shared by all active seed files.

| Input status | Handling before startup |
| --- | --- |
| Required save field is missing or null | Reject. Do not complete with random or default values |
| Missing nullable save field | Allow. Keep existing presence management and be null in GraphQL projection |
| Nullable saved fields are explicitly null | Allow |
| Violation of save field type, scalar constraints, and ID rules | Rejected. Do not conveniently convert strings/numbers |
| Inclusion of unknown fields, calculated fields that are not to be saved, etc. | Reject |
| Automatic relation/calculation resolver result field is not in seed | itself is not missing. Separately inspect the corresponding IR/required resolver |
| Missing reference of owner relation/cardinality violation | Rejected after importing all models |

It is not required to "write all non-null fields in SDL to JSONL". Distinguish between save slot and resolver result using IR.
On the other hand, it cannot be proven at startup that all paths of a dynamic resolver always return non-null.
The violation will be handled by the existing executor as a GraphQL error/null propagation during execution.

Rejects duplicate IDs within the same model, reads records for all models, and then inspects relations.
Maintain the JSONL line order as the existing registered order, and do not change the stable order of find and numbering reservation.
Prevent "reference to unread model" from failing due to seed order.
Do not listen before the inspection is completed. serve uses the inspected store and does not reread the seed file after startup.
The diagnostic shows the logical file/line/model/field and the reason for the fix, and does not reveal the raw record, ID value, or absolute path.

## 7. update: B/U/N and `.dist`

### 7.1 Criteria for comparison

- B: The last pristine candidate that the CLI installed at the active path or whose identical bytes it confirmed as adopted.
- U: User's current file bytes observed at start.
- N: New candidate bytes definitively generated from this SDL/IR/API.

`update-state.json` records each coordinate and relative path, the identity of B, the last presented N, and the CLI identity that owns a `.dist` proposal. Its closed schema classifies entries as active or orphaned, shares a generation identity with the snapshot, and rejects inconsistencies.
This identity is management metadata for edit detection and accident prevention, not a signature or third-party authentication mechanism.

| Status | Behavior |
| --- | --- |
| U = B | Replace U with N and advance B to N only after success |
| U = N | Treat the same candidate as already accepted and proceed from B to N without creating an extra dist |
| U ≠ B, N = B | Retain user edits. There is no need for a new dist because there is no change in the new candidate |
| U ≠ B, U ≠ N, N ≠ B | Keep U and suggest N to `.dist` in the same path |
| New coordinate, path not used | Exclusively install new candidate |
| New coordinate but path conflicts with existing file/orphan | Reject entire before changing |
| Missing active management file, missing/inconsistent B/management information | Reject without treating it as unedited. Do not recreate without permission |

B cannot be advanced to N just by exporting it to `.dist`. This prevents subsequent updates from mistaking edited U as unedited.
If the user adopts `.dist` so that U equals N, `check` can recognize that state.
A user-authored migration is also valid when it passes `check`.
It does not make U=N a starting condition, and protects U as a user edit even after the original migration.

Updating to the same N is idempotent. Do not create timestamps or generation
changes that produce an otherwise unnecessary diff. If the user deleted the
previous dist, the CLI may propose the same N again, but must not confuse that
case with a missing active U. Replace an existing dist only when its current
bytes match the CLI output recorded for it. If an edited or unmanaged dist
occupies a candidate output path, reject the update before changing any other
file. Never delete dist automatically. Its mere presence neither fails `check`
nor includes it in execution or export. If a retained `.dist` is not the latest
N, identify it clearly as an old proposal so it cannot be mistaken for the
current schema migration candidate.

### 7.2 Schema difference/orphan/partial migration

Report added, removed, and changed fields, arguments, models, constraints, and
relations by comparing with the schema snapshot. Do not guess renames; represent
one as removal of the old coordinate plus addition of the new coordinate.
Retain a removed resolver or seed as an orphan and exclude it from the current
IR execution set. If a later coordinate reuses the same path, report a conflict
for the user to resolve; never silently attach the old file to new authority.
An orphan explicitly deleted by the user may be removed from management during
the next update. Keep that distinct from rejection of a missing active file.

`update` validates the type and generation consistency of N itself, but an old
U that no longer type-checks, or incomplete existing seeds, does not prevent
placing the candidate. This lets the user receive the new type declarations
and dist before completing migration. Invalid settings or SDL, ownership
conflicts, and I/O or resource-limit failures are still rejected before
placement. Preserve existing seeds and `mock.luau`; create an empty JSONL file
for a new model, and let the subsequent `check` determine readiness.

### 7.3 Deployment and recovery

Retain the bytes and identity observed at startup through installation, and
recheck each target and its management metadata immediately before installing.
Reject concurrent edits, parent or final-component symlinks, FIFOs, and moves
outside the output scope. Build the candidate in private staging and treat the
generated area, updated resolver, dist, new seed, and management metadata as
one rollback unit. Do not implement replacement by renaming the old directory
or file to a backup, installing a candidate, deleting it, and regenerating it.

If a replacement target shares an inode with another observed input, including
settings, SDL, seed, retained modules, or dist, a rename would change that
other input's `ctime` relative to its starting observation. Reject this as an
ownership conflict before any write. Operations that do not modify the shared
inode, such as adopting an unmoved U=N, are exempt from this rejection. At most
1,024 logical files are managed, and temporary storage holds no more than one
candidate and one backup per changed target. These bounded old/new generation
copies exist for rollback; they do not create extra managed targets to evade
the limit. The transaction manifest and recovery data, including the larger of
the old and new generated-management payloads, must fit within 16 MiB total;
the manifest is also limited to 1 MiB and recovery data to 4 MiB. Do not begin
installation unless the pre-install manifest fits.

On failure, continue restoring the remaining targets even if one restoration fails. Preserve the primary cause, every recovery failure, candidates, and recovery material; never treat a missing or changed backup as successful restoration.
Set a clear commit point, and do not confuse failure to clean backup after commit with rollback before commit.
It does not claim to be a completely atomic update that includes multiple renames and even power off. Incomplete generations are detected and rejected by subsequent commands.
Persist transaction status when update or recovery is incomplete, preventing `check`, `serve`, `export`, and the next `update` from accepting an intermediate generation.
This status is also checked at the start/end of a read. References that only read settings/SDL are an exception that do not claim readiness.
Do not recursively delete the remains of the recovery process or user files in the name of cleanup.

## 8. Common load boundaries and references

```text
directory: evaluate configuration Luau once → load external SDL / active modules / JSONL
bundle:    read closed TOML → obtain evaluated settings / embedded SDL / modules / seeds
                  ↓
      common Mock IR → type/module initialization/all-seed inspection → owned inspected snapshot
                  ↓
           check / serve / export
```
Pass the shared inspected snapshot to the next stage. Do not reopen a path after inspection and consume different bytes.
Avoid implementing separate JSONL/bundle storage values, modules, and GraphQL execution.
`reference` uses only the bounded input preparation required below, not the complete startup inspection.

| reference input | configuration Luau | resolver | seed | readiness check |
| --- | --- | --- | --- | --- |
| None / `--schema` | Do not execute | Do not read/do not execute | Do not read | Do not |
| `--config` bundle | Do not run. Check envelope/SDL | No execution/type checking | No store checking | No |
| `--config-dir` | Execute once in bounds to get SDL reference | Do not read/do not execute | Do not read | Do not |

Directory references also do not require completion of generated/update management information.
If settings and SDL are valid, the API remains available for inspection even when a resolver or seed is unimplemented or invalid.
Reject invalid configuration Luau or SDL. Only typed API definitions and IR are authoritative for reference output.

reference v2 includes `input_kind` (`common`/`schema`/`project`/`bundle`), `config_executed`,
Record `readiness_checked = false`. `config_executed` is true only for project input.
`mock_format` indicates the input format and is null for common/schema. Supported project/bundle formats are shown separately as common information.
Don't confuse API version, CLI build identity and format version. Do not output raw source/seed/absolute path in JSON.
The text also indicates whether or not the settings are being executed. Maintain TTY/pager/no-pager/pipe and output budget for ADR-0031.

## 9. Single TOML export

```bash
pythia-mock check --config-dir mock
pythia-mock export --config-dir mock --output mock-bundle.toml
pythia-mock serve --config mock-bundle.toml
```
Export converts the same owned snapshot that passed the equivalent of `check`
into `pythia-mock/v3`. The bundle contains its format and API versions,
evaluated server settings, logically named SDL source, digest, and vocabulary,
ready state and module source for every active coordinate, and normalized JSON
for every active seed. Reject a mismatch between bundled state and the state
produced by module initialization; do not trust one side selectively or allow
an unimplemented module through. Preserve integer precision, record
registration order, and SDL order, and never convert JSONL through `f64`.
External `types.d.luau` is unnecessary because identical declarations can be
derived from the embedded SDL and API.

Do not include settings Luau source, Lua bytecode or closures, external path
dependencies, dist files, orphans, snapshot/update management metadata, or
Mutation state created while starting the server. Do not re-run configuration
Luau when serving a bundle. Resolver source and seeds are part of the
distribution; export does not automatically remove secrets supplied by the
user.

Emit the same bytes from the same evaluated settings, logical SDL/order, resolver bytes, seed, and renderer.
Output contains no timestamp- or working-directory-derived differences. This is
not a claim that arbitrary Luau programs are mathematically deterministic.
Export only the CLI binary and bundle to another directory and verify that it can be executed offline without the original project/SDL.

The output is a new file outside the editable project. Never overwrite an
existing file or input source. Complete the bounded render in a private
temporary file on the same filesystem, then publish it exclusively. If another
actor creates the destination after export begins, preserve that file, fail
the export, and leave no partial TOML at the final path.
Do not update project, adopt dist, or supplement seed when export fails.

## 10. Resource limits and execution security

Do not use format changes as a reason for expanding resource limits. Check the following as a closed upper limit for the implementation.

| Target | Upper limit/handling |
| --- | --- |
| Settings Luau source | 256 KiB |
| Setting return data | depth 32 / node 100,000 / owned bytes 16 MiB, SDL reference count 64 or less |
| All SDL | Total 128 KiB, source 64, type 256, member 4,096, model 64, script 256 or less |
| resolver module | Entire module 32 KiB, type check wrapper individual 1 MiB/total 16 MiB |
| active JSONL | raw total 256 KiB. Do not reset budget to individual files |
| canonical bundle | 256 KiB. Apply the same normalized byte limit to directory check/serve |
| generated type/snapshot/update management information | total 16 MiB. Generate and read bounded |
| update management file | 1,024 or less including active/orphan/dist/management information. Do not delete without permission when exceeding the limit |
| store/host operation/GraphQL output | Maintain the upper limit of ADR-0030/0031 |
| Settings/module initialization VM | VM memory 16 MiB, each evaluation deadline 500 ms, pcall simultaneous re-entrancy 16 steps or less |

Inspect the bundle byte of the directory using a counting sink, etc., and do not require the creation of a temporary export artifact.
If you want to expand the capacity, make separate measurements and design decisions, and do not allow "unlimited because the file is divided".
Do not implement an implementation that checks the number, depth, and aggregate bytes only after constructing a large DOM.

Apply the existing VM admission for Luaur's process-global constraints to the entire lifetime of the VM and handle, including configuration/initialization.
The wait is also bounded, and the deadline of the request is not reset before and after initializing the resolver module.
Memory/deadline/reentrant excesses caught by pcall are also retained as sticky failures, and configuration acceptance and mutation commit are rejected.
VM recovery after an inspection failure, recovery on the same server, and rollback of both models and numbering will not be regressed.
VM budget and process RSS limit are not the same. The existing input/processing budget including compiler/typechecker will also be maintained.

File/path/rename protection is a necessary product contract to prevent destruction of edited data, not a reinstatement of obsolete SSH retrieval defenses.

## 11. Implementation order and acceptance gate

### 11.1 Work packages

1. WP-MP0: Check 0031 and worktree at the start of implementation and create a closed inventory of old CLI/format/fixture/inspection.
2. WP-MP1: Implement the configuration VM, external SDL loading, typed modules, declaration generation, and shared owned snapshot.
3. WP-MP2: Connect `init`, `check`, `serve`, and JSONL, then first regress the existing real HTTP, VM, and transaction behavior.
4. WP-MP3: Implement B/U/N, schema difference, dist/orphan, placement transaction and recovery.
5. WP-MP4: Implement reference input boundaries, portable export, and offline/HTTP equivalence.
6. WP-MP5: Complete removal of legacy routes, shared validation, independent review, usage guidance, migration, and recovery records.

### 11.2 Closed acceptance inventory

The initial state of all IDs is Required / Not run. Do not reuse existing MR/RSM Passed evidence for MP.

| ID | Required validation |
| --- | --- |
| MP-01 | init creates module, type, management information, and empty JSONL for each role/coordinate. Also close and enumerate nested/manual |
| MP-02 | Two domains, root rename, reserved name, case/path/type name conflict. Local differences and stable names for extraneous field additions |
| MP-03 | Verify that IR/API-derived types and read/write/nested permissions are enforced, including rejection of nullability, presence, argument-type, and return-type mismatches. |
| MP-04 | Settings Accept Luau calculations and reject unknown keys/invalid values/function/cycle/metatable/sparse arrays |
| MP-05 | Inspect configuration-VM behavior for loops, excessive allocation, `pcall` levels 16 and 17, captured failure, and global leakage; stop, recover, and run subsequent work |
| MP-06 | Settings are evaluated only once in the command, no Lua handle remains after ownership, and no source is reloaded after inspection |
| MP-07 | Relative/absolute SDL, declaration order, duplicate files, SDL changes, cwd independent. Do not fallback snapshot to authority |
| MP-08 | Check CLI required/exclusive/duplicate/unknown options, help/version, old format/alias rejection in real binary |
| MP-09 | Replace U when U=B and advance B only after installation. A no-change update produces no unnecessary byte or generation differences. |
| MP-10 | Keep edited U and suggest N.dist. Do not create unnecessary new proposals when N=B |
| MP-11 | Cover U=N adoption and its unique migration check; ensure `.dist` is neither executed nor exported and its presence alone does not prevent startup. |
| MP-12 | Cannot advance B with dist generation. Rejecting iterative/subsequent schema changes, edited/unmanaged dist conflicts before whole change |
| MP-13 | New path collision, active U/B/management information missing, invalid generation, symlink/FIFO not considered unedited |
| MP-14 | Delete/rename orphan retention/re-introduction conflict/manual orphan organization. Retain existing seed/config bytes |
| MP-15 | Simultaneous editing, each move/later failure, backup missing/changed, restoration of all targets continued, cleanup failure after commit and recovery data retained |
| MP-16 | Keep successful update placement separate from readiness. Permit candidate generation when old U or seed data is invalid, while `check`, `serve`, and `export` still reject the project. |
| MP-17 | JSONL empty file/CRLF/terminal, blank/BOM/non-object/duplicate key/integer precision/depth/aggregate upper limit |
| MP-18 | Reject required saved field missing/null/constraint/unknown/calculated field/same model ID duplicate before listening. Allow nullable missing |
| MP-19 | Reference/cardinality/uniqueness check after loading all models. Relation, line order and numbering reservations that do not depend on input order |
| MP-20 | check does not call listen/resolve itself, but performs the same ready/type/module initialization/seed judgment as serve/export |
| MP-21 | Closed module state and function, independent parsing and expected type, wrapper-escape rejection; casts cannot bypass Rust boundaries |
| MP-22 | Common deadline for module initialization and resolve, fresh VM, non-shared closure between requests, return after initialization failure |
| MP-23 | Maintain snapshot/root rollback/numbering/cancellation/commit/nested post-commit failure contract for existing 7 model operations |
| MP-24 | Execute only the settings when inputting the reference project. Invalid module/seed/unimplemented can be referenced, settings/SDL invalid settings are rejected |
| MP-25 | Other reference inputs do not execute user code; verify v2 format, execution disclosure, confidentiality, TTY/pager behavior, JSON, and output limits |
| MP-26 | Required contents and excluded items for export, re-derivation of self-contained declaration, accuracy, source/record order, digest preservation |
| MP-27 | Do not publish the final file or change the input project due to export inspection/IO/output conflict/upper limit failure |
| MP-28 | Definitive bytes of export, no difference derived from cwd/time. No automatic inclusion of random numbers or running state |
| MP-29 | Offline real HTTP startup and operation with only binary and bundle, no original SDL/project, no network |
| MP-30 | Query/Mutation/relation/GraphQL error/rollback/restart initial state is the same in directory/bundle |
| MP-31 | Test every resource limit at and beyond its boundary across normal Rust 1.85, Mock Rust 1.95, and stable Rust; preserve ABI 6, core dependency constraints, and every existing SDK exact/quality check. |
| MP-32 | Complete reviews A and B, concrete assertion traces for every ID, common validation, and records of untracked files, unexecuted CI, migration, recovery, and measurements. |

Gate MP-A covers MP-01 through MP-08, MP-B covers MP-09 through MP-16, MP-C covers MP-17 through MP-23, MP-D covers MP-24 through MP-30, and MP-E covers MP-31 and MP-32 plus the complete trace.
Review A independently checks settings/types/seed/GraphQL and store semantics, and Review B independently checks editing ownership/simultaneous changes/recovery/export/reference/VM resource boundaries.
Do not hide unresolved findings and unexecuted environments and mark them as Passed, including re-reviewing them after fixing them.

Prioritize focused tests during development, and consolidate time-consuming full tests into the final stage.
Incorporate the new contract into the final common validate, and do not treat it as completed by simply deleting the existing command or posting the old number of successes.
Preserve existing required debug and release tests for `pcall`, memory, cancellation, root rollback, VM recovery, and worker collection.
Also preserve the 400-line limit, the English prose rule from ADR-0043, generated-file immutability, ownership checks, and quality checks.

## 12. Handover to subsequent implementation

When implementing, add a new program section to architecture/handoff/conformance and synchronize the usage procedure with the current command in README.
This design addition does not change those implemented descriptions or the goals of ADR-0031.
When requesting implementation to a new agent, pass the entire text of goal prompt.
Just preparing a goal does not mean starting or completing implementation or passing the MP gate.

Subsequent implementations include test/assertion of all MP-IDs, execution command/results, in `docs/mock-project-workflow-validation.md`.
Document initial failures and revalidations after corrections, independent reviews, legacy route removal inventory, and critical untracked artifacts.
Specify whether OS/remote CI is not running and measure build/rebuild/startup/RSS and update/export of large projects.
No new performance SLO or editor-completion claim may be made without
measurement.

The basic workflow is `init` → edit configuration, resolvers, and JSONL → run `update` after schema changes → manually merge `.dist` or keep an independent implementation → `check` → `serve`. Add `export` only when distribution is required. Also document manual migration from existing TOML and recovery examples.

At planning time, Lab01 HEAD was `bc9661e1763f7659f76cd7d8dba17400a738b9e3`, with uncommitted ADR-0031 changes.
This is not a fixed revision for subsequent tasks. Measure at the beginning of the implementation and do not revert existing changes or assume completion.
live application sources are not subject to changes in this ADR, and stage/commit/push/publish/tag will not be performed without explicit instructions.
