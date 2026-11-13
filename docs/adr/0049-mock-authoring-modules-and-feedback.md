# ADR-0049: Actionable Mock authoring feedback

Status: Accepted

Date: 2026-09-24

Implementation: Implemented. AF-08 through AF-11 and AF-14 through AF-16 pass
locally on macOS, including both independent review tracks and the complete
product validator. AF-01 through AF-07 and AF-12/AF-13 are retired by the scope
amendment rather than counted as successful tests.

Planned successor: [ADR-0051](0051-mock-contract-validation-without-completion-state.md)
removes resolver/hook completion state and the unfinished-inventory feature at
its implementation cutover. AF-11 and the inventory-specific parts of AF-14 and
AF-16 then become historical acceptance, not failed or never-executed tests.
Terminal refinement, safe diagnostics, reference navigation, and check v3
remain required. The successor's version table governs its clean break;
the implementation results below remain evidence for this ADR's own contract.
Until that cutover, this ADR describes the current executable.

Scope amendment: Project-local `require` and all dependent library features
are removed from this ADR, not waiting for implementation under it. The
published-dependency blocker is historical. The remaining work is authorized
without waiting for a multi-source checker release.

## 1. Context and decision history

An authoring run after ADR-0047 implemented 26 resolvers in the sibling
`graphql` project. Inline predicates and multi-key ordering worked without
the earlier annotation workarounds, and representative HTTP operations mostly
worked on their first execution. The reported 21-minute run reused some prior
SDL and documentation decisions; it is not a controlled speed comparison.

The remaining feedback identifies repeated authentication and pagination code,
nullable guards needing redundant assertions after `ctx.fail`, generic
`structural type unavailable` diagnostics, large reference dumps, and only the
first unfinished resolver being reported. Target checks reportedly took several
seconds. These observations motivate focused authoring improvements, not a
general application framework or a business-testing platform.

### 1.1 Considered and withdrawn: project-local require

The original decision included typed shared modules, a `lib/` tree, static
alias/relative imports, dependency-aware checking, per-VM loading, portable
module export, and a commented init sample with usage examples.

The release-only feasibility work found that the adopted public
`luaur-rt 0.1.8` safe checker API accepts one source and does not provide the
safe module-resolution, cross-file typed-consumer checking, and diagnostic
module identity needed for that contract. This is a limitation of the adopted
published API, not a claim that the Luau language cannot type-check modules.
Checking files independently does not establish the type boundary between them.
Missing detailed diagnostic kinds alone was not a blocker: safe general
diagnostics remain an acceptable fallback.

An earlier proposal to patch/vendor the dependency was withdrawn in favor of
unmodified public releases. A Chigiri-managed restricted interface or
checker-source linking scheme was also considered, but would add another
module-adaptation responsibility. Shared code is useful, but authors can
implement the mock without it. Keeping the product simple takes priority.

This revision therefore removes the entire shared-module feature rather than
keeping the ADR blocked on a future release. No library loader, import grammar,
graph capture, module cache, library target, export extension, `lib/example.luau`,
or `reference runtime modules` is delivered by this ADR. The previous sample
requirement is withdrawn with the feature. Existing unsupported-loader
behavior remains unchanged; there is no unchecked/runtime-only fallback.

Preserve the release probes, source identities, timings, reviews, and blocked
handoff in the validation record as
historical evidence. Do not rerun that dependency gate as a completion
condition or revive modules automatically when a release appears. Any future
shared-module proposal requires a separate design decision. Existing document
filenames retain their original module-related slugs to preserve links.

### 1.2 Retained scope

Continue with all module-independent authoring improvements:

1. Correct terminal-flow refinement for genuine `ctx.fail` calls.
2. Safe, specific diagnostics where the available facts support them, with an
   honest general fallback elsewhere.
3. Scoped reference guidance attached to relevant diagnostics.
4. An explicit inventory of unfinished resolvers and enabled hooks.

Retain measurement of the affected commands, not a daemon, cache, latency
promise, or business-scenario test framework. Existing partial work is a
baseline to review and complete, not work to discard or an automatic pass.

## 2. Supersession and preserved boundaries

At implementation cutover, this ADR takes precedence only as follows:

| Earlier contract | Narrow change |
| --- | --- |
| [ADR-0041](0041-mock-reference-topics-and-navigation.md) | Reuse existing named reference selections in diagnostic navigation; do not add a module topic. |
| [ADR-0042](0042-mock-typecheck-batch-and-init-performance.md) | Preserve batching, isolation, original-source validation, resource controls, and the prohibition on dependency patches. No multi-source checker integration is required. |
| [ADR-0044](0044-composable-agent-ready-mock-authoring.md) | Add unfinished-entry inventory and improve generated guidance, not a second readiness authority. |
| [ADR-0047](0047-mock-authoring-type-and-output-boundaries.md) | Extend checker-only flow handling and safe feedback within published APIs; version the changed check report. |
| [ADR-0048](0048-chigiri-brand-and-single-binary-cli.md) | Retain one `chigiri` executable and both namespaces; add only the Mock check option below. |

Luau remains a bounded subset. No filesystem, network, process, environment,
package manager, shared-module loader, or dynamic-code capability is added.
Public SDL remains authoritative for stored, relation, and virtual fields.
Preserve phase permissions, clocks, provenance, private output protection,
request snapshots, atomic commit, and existing ownership/recovery protections.
Normal Rust libraries and generated consumers retain MSRV 1.85 and ABI 6;
CLI/Mock retain their separate Rust 1.95 lane and dependency boundary.

Use only unmodified, non-yanked crates.io releases for luaur and its required
components, retaining exact direct versions and registry checksums. Do not add
vendor, patches, forks, Git dependencies, copied checker internals, unsafe
wrappers, or VM/solver changes. This work does not require a dependency upgrade
or removal of existing unrelated vendor sources. Missing richer native
diagnostic data calls for the defined fallback, not a new dependency blocker.

## 3. Terminal flow and diagnostics

### 3.1 Correct refinement after business failure

Do not make redundant `assert(session ~= nil)` the supported fix for a genuine
terminal guard. The pinned checker can retain nullable types after an ordinary
`never`-returning member call, while an explicit terminal return narrows them.
For host contexts, both of these patterns must check without added assertions:

```luau
if session == nil then
    ctx.fail({ code = "UNAUTHENTICATED", message = "Authentication required" })
end
local userId = session.userId
```

```luau
if session == nil or session.revoked or now == nil or session.expiresAt <= now then
    ctx.fail({ code = "UNAUTHENTICATED", message = "Authentication required" })
end
local userId = session.userId
local timestamp = now
```

Use a minimal checker-only terminal-control-flow transformation for proven
host `ctx.fail` calls. Reuse lexical binding analysis and source maps from
ADR-0047. Preserve original runtime source, evaluation order, varargs scope,
single evaluation, nested function boundaries, and both span endpoints.
Standalone original-source parsing/compilation remains mandatory.

Do not terminalize an unrelated method named `fail`, a shadowed/reassigned
context, or continuation after `pcall` merely because its callback can fail.
Recognize generated host context parameters, including typed same-file helper
parameters and lexically proven aliases. Do not require arbitrary interprocedural
inference for untyped wrappers. Real nullable accesses, invalid error payloads,
wrong arguments, and wrong return types must still fail. Do not inject runtime
assertions, suppress diagnostics, widen types, or patch the native solver.

### 3.2 Specific, safe feedback

For reliably identified errors, distinguish nullable access, type/return
mismatch, invalid descriptor, and unavailable phase capability as supported by
trusted facts. Report the actual logical source and original position; do not
invent a GraphQL coordinate for a hook, configuration, or non-coordinate error.

Use structured native diagnostic kinds when exposed by the selected release,
or facts independently established by validated AST/contract information, not
message-string heuristics. A location alone does not prove a possibly-nil
access or a specific mismatch. If the public API supplies only a location and
free-form message, retain the verified location and use fixed host-owned
general wording unless another trusted fact proves the classification.

Retain existing `MOCK_TYPE_MISMATCH` for proven mismatch sites and
`MOCK_CONTRACT` for otherwise unclassified native contract failures.
`structural type unavailable` remains an honest fallback. Do not promise a
nil-specific explanation when the checker does not expose enough information,
add speculative cause codes, reconstruct its solver, or guess from nearby
syntax. Render safe type categories only when proven. Do not recommend changing
a correct neighboring expression or infer business intent from a member name.

All diagnostics remain bounded and sanitized. Do not copy compiler messages,
source excerpts, string literal values, tokens, request data, physical paths,
or arbitrary exception text. Preserve the existing runtime log and generic,
correlated HTTP internal-error contracts. Human text, JSON, generated guidance,
and reference navigation share the same classification authority.

### 3.3 Scoped reference navigation

Attach `references`, a bounded list of structured `{ topic, name }` selections,
to check/inventory diagnostics when the current registry proves a relevant
model, type, resolver, hook, or runtime selection. Verify both that the selection
exists and that its relationship to the diagnostic is established. An enclosing
resolver or hook may supply a proven contract reference even when the native
error's finer classification is unknown. Do not guess a model from source text.

Use at most four distinct selections per diagnostic, sorted by topic and name.
Each object has exactly those two string fields. Use an empty list when none
is proven. Restrict topics to the existing named `models`, `types`,
`resolvers`, `hooks`, and `runtime` selectors; unknown or unavailable
selections are omitted. Names come from the captured registry, not arbitrary
source values. Apply existing identifier and diagnostic output bounds.

Render equivalent bounded text guidance. For example, inside the project:

```text
chigiri mock reference models Session --config-dir .
chigiri mock reference runtime errors --config-dir .
```

Use the actual selected input mode for runnable guidance. Do not echo a physical
config path or blindly suggest `--config-dir` for bundle input; use an explicit
safe path placeholder and explain it when a path is needed. References must be
available without successful readiness. Do not evaluate project configuration
again merely to build suggestions.

Keep whole-reference output optional; do not dump model catalogs into errors.
No new reference topic or full/topic reference shape is needed. Generated
`AGENTS.md`, CLI help, and the workflow guide explain targeted discovery and
the distinction between inventory, scoped checking, and full readiness.

## 4. Unfinished-work inventory

Add exactly this authoring mode:

```text
chigiri mock check --list-unimplemented --config-dir schema --format json
```

`--format text|json` retains text as default. The option requires project input
and conflicts with `--target`; it is unavailable for portable bundles. Invalid
combinations remain usage errors. This is enumeration, not a partial full check.

Capture settings, schema, ownership, generated declarations, and the active
layout using existing verified reads and locks. Enumerate every active resolver
and enabled hook in deterministic logical-path order. Inspect descriptors using
bounded original-source compilation, contract checking, and initialization in
the existing sandbox. Never invoke `resolve` or `run`, validate business
scenarios, or make unrelated seed validity a prerequisite. Disabled hooks and
orphan files are not unfinished entrypoints.

The closed JSON report is `chigiri-mock-unimplemented/v1`, with exactly
`format`, `complete`, `readiness_checked`, `unimplemented`, `unclassified`,
and `diagnostics`. Each entry has exactly `file`, `kind` (`resolver` or
`hook`), nullable GraphQL `coordinate`, and nullable `hook_slot`.
A resolver has its coordinate and a null hook slot; a hook has its canonical
slot name and a null coordinate. Diagnostics use Section 5's check-v3 shape.
`readiness_checked` is always false; do not add a `valid` field.

Exit zero when enumeration is complete, even if unfinished entries remain.
Missing, malformed, unsafe, failed-initialization, or otherwise unclassifiable
active scripts make `complete=false` and the command fail. Return known entries
and identify unclassified scripts where possible, rather than silently omitting
them. Continue through independent scripts within existing aggregate budgets.
Cancellation or a hard limit stops work and reports incompleteness, including
remaining known active entries as unclassified where possible.

Configuration/schema/ownership failure may prevent establishing any inventory;
return an incomplete report with safe diagnostics, not an empty success.
Verify captured inputs before reporting success. Input changes, locks, resource
limits, or cancellation cannot be downgraded into successful enumeration.
Lists are de-duplicated and ordered by logical path; diagnostics use a stable
logical-source/position order. Preserve existing source, wrapper, VM, deadline,
and output bounds, including the aggregate limits across all inspected scripts.
Bound report cardinality by the admitted active inventory; if output cannot be
represented within its limit, fail explicitly rather than silently dropping
entries and claiming completeness.

Keep the direct-output contract, including successful BrokenPipe handling.
An empty complete inventory means only that no active descriptor declares
itself unimplemented. Full `check`, `serve`, and `export` still require their
existing complete type, seed, initialization, and readiness validation.

## 5. Versioning and compatibility

This is the explicit replacement for the former staged version plan. It
authorizes public inventory and scoped diagnostic navigation now; no further
module release or staged-scope amendment is required.

| Surface | Current baseline | This decision |
| --- | --- | --- |
| Project | `chigiri-mock-project/v6` | Unchanged; no library layout. |
| Bundle | `chigiri-mock/v8` | Unchanged; no embedded module graph. |
| Host API / capabilities | v7 / v2 | Unchanged; no loader capability. |
| Full / topic reference | v9 / v5 | Unchanged; reuse existing selectors and shapes. |
| Check | `chigiri-mock-check/v2` | v3: structured reference selections and honest nullable source/coordinate fields. |
| Unfinished inventory | None | `chigiri-mock-unimplemented/v1`. |
| Runtime log | `chigiri-mock-log/v3` | Unchanged; no import origins or structured navigation fields. |
| Update state / schema snapshot | v7 / v7 | Unchanged. |
| SDL vocabulary / generated Rust ABI | v3 / ABI 6 | Unchanged. |

Check v3 retains the top-level fields `format`, `scope`, `valid`,
`readiness_checked`, `targets`, and `diagnostics`, with existing meanings.
There is no dependency expansion, `checked_targets`, or `import_context`.
Target grammar and the distinction between target success and full readiness
are unchanged.

Each v3 diagnostic retains `severity`, `file`, `coordinate`, `member`,
`line`, `column`, `embedded`, `phase`, `cause`, `expected`, `actual`,
`hint`, and `message`, and adds `references` as defined in Section 3.3.
Use null for `file`, `coordinate`, `member`, or the paired `line`/`column`
when that information is genuinely unavailable, not fabricated sentinel
values. Known source positions stay one-based. `embedded` retains its existing
nullable position shape. Hook slot identity belongs in inventory entries or
an existing honest member identity, not a fabricated GraphQL coordinate.

Specify exact fixtures, bounds, and variants before implementing report
consumers. Keep one current producer/reader for changed report contracts, with
no v2 compatibility switch or mixed-version acceptance. Shared internal
diagnostic data must not leak new fields into unchanged runtime log or
reference wire shapes.

The removed module-only version increments are withdrawn, not reserved or
emitted. Preserve valid existing project and bundle input. No fresh-destination
project migration is required for this amendment. Regenerate owned guidance and
fixtures through canonical producers and preserve current `update`, `.dist`,
transaction, recovery, and user-source ownership rules. Never delete an existing
user `lib/` directory merely because this feature was withdrawn. Reconcile
version collisions explicitly if another accepted change consumes check v3.

## 6. Work packages and acceptance

Preserve earlier AF identities so historical tests and evidence remain
traceable. AF-01 through AF-07 and AF-12/AF-13 are retired module-only criteria,
not passes, waivers, blockers, or requirements to implement later in this ADR.
The module clauses of retained rows are removed; their remaining obligations
are defined below.

Likewise, AF-WP1 (module integration) is retired. Continue with these packages:

1. AF-WP0: reconcile the reduced scope, existing partial implementation,
   historical evidence, exact report fixtures/version inventory, and usable
   release-build timing baseline. No module dependency probe is required.
2. AF-WP2: complete terminal refinement, safe diagnostics, scoped reference
   navigation, and the unfinished-entry engine using the published APIs.
3. AF-WP3: expose the inventory command and revised check report, update
   generated guidance/help/workflow docs, and complete integrated acceptance.
   No library sample or module contract cutover belongs here.
4. AF-WP4: complete regression gates, comparable timing measurements, independent
   architecture/security and regression/workflow reviews, fixes and re-review,
   and a final-source evidence record.

The active completion set is AF-08, AF-09, AF-10, AF-11, AF-14, AF-15, and AF-16.
Existing partial evidence may be reused only for unchanged, identified source
and applicable assertions; public delivery and final review remain required.
The sibling `graphql` directory is motivation, not a test dependency or edit
target. Publishing this amendment does not establish any new test pass.

| ID | Required evidence in the reduced scope |
| --- | --- |
| AF-08 | Simple and compound genuine `ctx.fail` guards narrow without redundant assertions in resolvers, hooks, and typed same-file helpers. Shadowing, reassignment, captured aliases, local type shadowing, nested scopes, protected calls, invalid payloads, actual nil accesses, varargs, original runtime behavior, and both span endpoints have negative/regression coverage. |
| AF-09 | Trusted nil/type/descriptor/phase facts produce correct original positions and safe concrete guidance where available; unclassified native failures retain a tested honest fallback. No message parsing, invented type facts, neighboring-expression blame, or leaked values/physical paths in text, JSON, runtime logs, or HTTP internals. |
| AF-10 | Bounded, deterministic diagnostic references resolve in the existing registry and are relevant, including during incomplete readiness. Unknown suggestions are omitted; text and JSON agree. Guidance works with the selected input mode without exposing physical paths, repeating configuration evaluation, dumping catalogs, or advertising modules. |
| AF-11 | Inventory reports multiple unfinished resolvers and enabled hooks, deterministic ordering, disabled/orphan exclusion, and known plus unclassified entries beside malformed, unsafe, failed-initialization, or missing scripts. Bodies never execute; invalid seeds do not block enumeration. Configuration failure, aggregate limits, cancellation, and input changes fail honestly. Empty/nonempty completion and exit/readiness semantics are exact. |
| AF-14 | Exact check-v3 and inventory-v1 fixtures cover fields, nullability, bounded references, usage failures, empty and incomplete reports, output limits, and real BrokenPipe behavior. Existing target grammar and full readiness remain distinct; no library targets or expanded-target fields appear. One binary and both namespaces are retained. |
| AF-15 | Registry-only pinned luaur dependencies, no added vendor/patch/fork/Git or unsafe workaround, no loader/sample/module export, and unchanged retained formats are verified. Current-project init/update/check/reference/serve/restart/export, ownership, recovery, permissions, rollback, and secrecy regressions remain green. MSRVs, ABI 6, dependency direction, formatting, Clippy, rustdoc, generated exactness, implementation rules, and the final full product validator pass. |
| AF-16 | Comparable release timings cover full check, an existing resolver/hook target, inventory, and init with cold/repeated runs and source/binary identities. Inventory's unavailable old baseline is explicit, not zero. Final evidence maps the active and retired AF IDs, final-source review resolutions, and unexecuted environments without claiming a controlled authoring speedup. |

Product regression tests prove these tool contracts. They do not require mock
authors to reproduce full production business-test suites. Integrated scenarios
need enough assertions to prove the changed boundaries, not a new scenario DSL.
Mark the ADR Implemented only after the active completion set and both reviews
pass. Retired module criteria must not keep the reduced goal blocked.

## 7. Consequences and non-goals

Authors keep some duplicated helpers in exchange for a smaller product
surface. Terminal refinement removes a real workaround; honest diagnostics and
targeted navigation reduce investigation; inventory makes remaining work
visible without competing with full readiness. None requires ownership of a
module type system or locally maintained upstream code.

Do not add `require`, a library sample, sequential cross-file type linking,
a `chigiri mock test` command, a scenario runner, a watch daemon, persistent
type-check caches, an LSP, runtime dependency downloads, a package manager,
hot reload, external communication, production persistence, or a background
worker. Do not relax limits to meet a timing target. Measure actual regressions
before choosing a later performance project; no absolute latency or authoring
time guarantee is part of this ADR.

ADR-0050 remains a separate assignment. Distribution, platform qualification,
publishing, and release claims are not implied by completion of this authoring
follow-up.
