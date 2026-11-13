# ADR-0051: Mock contract validation without completion state

Status: Accepted

Date: 2026-09-24

Implementation: Implemented and locally approved on macOS arm64; all review
corrections are verified and closed. See
the validation record and
Beads review `lab01-c43.2`.

## 1. Context and decision

The Todo quick start demonstrated a straightforward authoring workflow, but
also exposed redundant bookkeeping: authors must change a resolver's body and
separately declare `state = "ready"`. The declaration is not evidence that the
behavior is correct or complete. Either a person or an agent can leave the
declaration inconsistent with the implementation.

The existing flag detects untouched scaffolds during initial authoring and
after schema updates. However, a schema may intentionally include operations
whose behavior is not yet decided. Returning an explicit error is a valid Mock
implementation for those operations; their presence need not prevent unrelated
operations from serving frontend development.

Remove implementation-completion state from both resolver and hook modules.
Chigiri validates executable contracts, not business completeness. A module
whose entrypoint deliberately fails can satisfy its contract. The consequence
is intentional: an untouched operation is discovered when invoked rather than
by a startup refusal. Do not replace the removed declaration with another
completion heuristic or work-tracking feature.

This ADR defines the implemented clean-break cutover. The reported quick-start
experience is motivation, not a controlled benchmark.

## 2. Supersession and preserved boundaries

At implementation cutover, this ADR takes precedence only as follows:

| Earlier contract | Narrow change |
| --- | --- |
| [ADR-0030](0030-runtime-schema-mock-ir-and-directives.md) and [ADR-0032](0032-mock-project-layout-and-portable-export.md) | Remove completion-state admission, resolver descriptor state, and portable resolver-state metadata. Keep explicit authored behavior, typed modules, verified inputs, and edit-protected update. |
| [ADR-0036](0036-mock-luau-hooks-and-json-logging.md) | Remove hook descriptor state and portable hook-state metadata, not hook registration or request-local state. |
| [ADR-0044](0044-composable-agent-ready-mock-authoring.md) and [ADR-0047](0047-mock-authoring-type-and-output-boundaries.md) | Readiness remains full contract validation, with no declaration of business completion. Preserve scoped checking, typed output boundaries, and safe diagnostics. |
| [ADR-0049](0049-mock-authoring-modules-and-feedback.md) | Remove unfinished inventory and its dependent guidance, reports, and completion counts. Keep terminal refinement, safe feedback, scoped reference navigation, and check v3. |
| [ADR-0050](0050-portable-host-boundary-and-tiered-builds.md) | No host/build supersession. When both decisions are implemented, the host boundary supports the retained workflows without requiring the removed inventory. |

ADR-0049's AF-11 and the inventory-specific portions of AF-14 and AF-16 become
historical at this cutover, not failed or never-executed tests. Its old version
table is superseded only by Section 5 below. Preserve previous source
identities, reviews, timings, and acceptance records under their original
contracts; do not relabel their results as evidence for this ADR.

Keep public/private SDL boundaries, generated types, phase permissions, record
provenance, request snapshots, clocks, atomic commit, failure precedence,
secrecy, and resource controls. Request-local `ctx.state`, configuration state
declarations and initial values, `statePatch`, hook enable/disable settings,
and generated update-state metadata are different concepts and remain supported.
No dependency, MSRV, Rust ABI, or operating-system policy change is required.

## 3. Module and validation contracts

### 3.1 Closed descriptors without completion state

A resolver module returns a plain closed table with exactly one field,
`resolve`, holding the coordinate-specific typed function. A hook module
returns a plain closed table with exactly one field, `run`, holding its
slot-specific typed function. Keep the existing table/module representation,
function signatures, metatable prohibition, and per-initialization validation;
do not introduce bare-function modules or extra descriptor fields.

Remove `state` from generated resolver/hook descriptor types, public candidate
APIs, internal completion enums and checks, bundle entry metadata, and reference
type descriptions. Remove corresponding init/update completion counts,
including `unimplemented` and `unimplemented_candidates`; do not rename them
into a different implementation-progress measure. Placement events and schema
change reports remain available. A legacy descriptor containing `state` is
invalid in the new contract even when its value is `"ready"`.

The generated resolver body is:

```luau
ctx.fail({
    code = "NOT_IMPLEMENTED",
    message = "This operation is not implemented.",
})
```

Use the existing terminal `ctx.fail` capability, without a new runtime API,
reserved status enum, or generated output value. Preserve generated argument,
context, and result annotations. Each of the six generated hook bodies remains
a typed no-op returning `nil`, and all six registrations remain disabled at
init. Enabling such a no-op requires no separate completion declaration.

`NOT_IMPLEMENTED` is an ordinary public business-error code. Do not inspect it,
its message, or the function body to classify completion, infer an authoring
task, emit an unfinished warning, or decide admission. Authors may retain this
body intentionally, replace it, or use ordinary `error()` instead. A body that
can return a value of the wrong type still fails static contract validation.

### 3.2 Full validation, not behavior certification

Full `check` continues to validate configuration, SDL, active files, generated
identity, syntax, types, closed descriptors, bounded module initialization,
seeds, and existing structural/runtime admission constraints. Missing required
active modules, invalid modules, unsafe initialization, invalid seeds, drift,
and limit/cancellation failures remain errors. Disabled-hook and orphan-file
selection rules do not expand.

Module initialization may run inside the existing bounded sandbox. Never invoke
`resolve` or `run` to discover whether an implementation succeeds. A top-level
initialization error is still invalid; an entrypoint that deliberately errors
is not invalid for that reason. Do not add stub execution, TODO scans, source
text matching, or a replacement unfinished registry.

For otherwise valid inputs, the generated resolver bodies pass full `check`
and may be served or exported without editing. This is not a promise that every
SDL and seed set is structurally admissible, or that all operations succeed.
Existing required-reference-cycle and storage constraints remain enforced.

`readiness_checked = true` retains its meaning: successful validation of the
whole input for serving/export, not certification of business behavior.
Target checks retain `readiness_checked = false`; they validate only their
selected scope. Successful init/update still reports placement, not completed
full validation. `serve` and `export` perform their own full validation of the
current captured input; neither trusts a previous CLI invocation or skips
checks because the new scaffolds are admissible.

### 3.3 Existing runtime failure semantics

Invoking the generated resolver uses existing business-failure handling:
HTTP 200 with a GraphQL error carrying code `NOT_IMPLEMENTED`, the fixed public
message, and existing path/correlation metadata. It is not a successful field
value, automatic null/default result, or HTTP 501 response. The existing
request-wide failure and rollback contract applies, including discarding
earlier writes in the same Mutation request.

Ordinary `error()` still produces the existing sanitized internal failure,
`MOCK_EXECUTION_FAILED` with HTTP 500. Do not expose its raw message, downgrade
independent internal/resource failures, change protected-call behavior, or
introduce partial-success semantics. A failed invocation does not prevent a
later independent request from using implemented operations. Hook permissions
and phase-specific failure rules remain unchanged.

## 4. Authoring surface and guidance

Remove `chigiri mock check --list-unimplemented` from grammar and help, its
public inventory API/engine, and the `chigiri-mock-unimplemented/v1` report
producer/reader. Supplying the removed option is a usage error, not an alias,
empty report, or compatibility mode. Retain internal active-file inventories
needed for ownership and scoped validation; they are not completion tracking.

Update generated `AGENTS.md`, templates, reference descriptions, init/update
messages, examples, and the maintained quick start during implementation.
Explain that authors implement the operations they need, run scoped/full
checks, and observe intentional errors when invoking other operations. Never
claim that `check` proves all operations are implemented or that an erroring
resolver must be replaced before starting the server.

Keep current operating instructions accurate until that implementation lands.
Design documents may describe the accepted transition, but must label it
Planned rather than presenting new-format examples as current executable
behavior. Existing full/target check output remains bounded and direct, with
scoped reference suggestions, sanitized diagnostics, and BrokenPipe handling.

## 5. Versioning and migration

Use one current contract after cutover, without legacy readers, optional
ignored `state`, compatibility switches, or automatic source conversion.

| Surface | Current baseline | New contract |
| --- | --- | --- |
| Project | `chigiri-mock-project/v6` | `chigiri-mock-project/v7` |
| Portable bundle | `chigiri-mock/v8` | `chigiri-mock/v9` |
| Mock host API | `chigiri-mock-api/v7` | `chigiri-mock-api/v8` |
| Full reference | `chigiri-mock-reference/v9` | `chigiri-mock-reference/v10` |
| Topic reference | `chigiri-mock-reference-topic/v5` | `chigiri-mock-reference-topic/v6` |
| Update state | `chigiri-mock-update-state/v7` | `chigiri-mock-update-state/v8` |
| Schema snapshot | `chigiri-mock-schema-snapshot/v7` | `chigiri-mock-schema-snapshot/v8` |
| Check | `chigiri-mock-check/v3` | Unchanged |
| Unfinished inventory | `chigiri-mock-unimplemented/v1` | Removed |
| Capabilities / runtime log / SDL vocabulary / generated Rust ABI | v2 / v3 / v3 / ABI 6 | Unchanged |

The host API and reference increments identify changed descriptor contracts.
Project/bundle and management identities bind the new generation contract and
prevent old descriptors or generated declarations from being treated as current.
Check v3's shape and validation-scope meanings do not change; readiness simply
has no completion declaration to validate under the new input contract. No
global capability or runtime-log field is added. Normal Rust libraries and
generated consumers retain Rust 1.85; CLI/Mock retain their Rust 1.95 lane.

Reject old or mixed input identities before publication, replacement, or other
product writes. Do not silently strip state or upgrade an old directory through
`update`. The migration procedure is:

1. Keep the original project and initialize a separate, absent destination
   using the owned public SDL.
2. Adapt configuration/private storage there and use the new canonical update
   producer to generate the corresponding declarations and ownership metadata.
3. Deliberately copy/adapt user-owned resolver, hook, and seed content. Remove
   only descriptor completion fields; retain request-state declarations,
   `ctx.state`, `statePatch`, and hook registration settings as needed.
4. Do not copy old generated metadata or merely change a format string. Run
   scoped/full validation against the new project and export a new bundle if
   required. No bundle-to-project importer is introduced.

Current-format update retains hash-based base/user/next comparison, `.dist`
ownership, transactional installation, recovery, and preservation of edited
sources and seeds. New schema operations receive the same intentional-error
body and do not introduce a new completion gate.

## 6. Implementation packages and acceptance

Implementation used these named packages:

1. CV-WP0: capture the current baseline and freeze new descriptor, bundle,
   reference, report, and version fixtures; map superseded completion tests.
2. CV-WP1: remove completion state across generation, typed candidates,
   validation, runtime initialization, bundle handling, and reference output;
   perform the coordinated clean-break version cutover.
3. CV-WP2: remove unfinished inventory/counts and update authoring guidance,
   migration documentation, examples, and the maintained quick start.
4. CV-WP3: complete the acceptance below, retained product gates, independent
   architecture/security and regression/workflow reviews, fixes and re-review,
   and final-source evidence with unexecuted environments stated explicitly.

| ID | Required evidence |
| --- | --- |
| CV-01 | A fresh valid Todo project passes full check, serve, and export without resolver edits. Project and exported bundle invocations return the exact `NOT_IMPLEMENTED` business error rather than success or an internal failure. Initialization/check/export never invoke entrypoint bodies. |
| CV-02 | One implemented operation works beside an untouched one, and subsequent independent requests work after the latter fails. Ordinary `error()` remains a sanitized HTTP 500 internal failure with no raw-message leakage. |
| CV-03 | A later intentional failure rolls back earlier writes in the same Mutation request. Sticky failure, protected calls, independent internal/resource-failure precedence, and store recovery retain their contracts. |
| CV-04 | Generated resolver/hook types and runtime module validation agree on the one-field closed descriptor. Wrong signatures/returns, missing active modules, extra fields including legacy `state`, metatables, failed initialization, invalid seeds, drift, limits, and cancellation still fail. Disabled hooks remain excluded under existing rules. |
| CV-05 | All six hook scaffolds are no-ops and initially disabled. Enabling them needs no completion edit. Request-state declaration/initialization, patches, phase permissions, registration, and failure semantics remain intact. |
| CV-06 | Exact new project/bundle/reference/metadata identities reject old and mixed inputs without changing the old project or existing output. New-format descriptors never accept ignored legacy state. Fresh-destination migration regenerates metadata, preserves source ownership, and succeeds. |
| CV-07 | The inventory option, report, public inventory APIs, completion counts, and reference completion types are absent. Removed-option usage errors are exact. Check v3 retains its fields, target/full distinction, safe navigation, and direct-output/BrokenPipe contracts; init/update do not claim full readiness. |
| CV-08 | Schema update adds an intentional-error resolver while retaining edited code and seeds. Hash-based ownership, `.dist`, recovery, current-input validation, export, and restart semantics remain covered. Stub additions alone do not prevent serving. |
| CV-09 | Canonical fixtures, examples, generated guidance, quick start, and documentation match the new contract. Retained refinement, secrecy, resource, dependency/MSRV, ABI, formatting, Clippy, rustdoc, generated-exactness, implementation-rules, and full product-validator gates pass; both reviews close on identified final source. |

All CV rows have local macOS arm64 evidence in the validation record. Old
AF/AB/CH passes do not establish CV acceptance. Regression tests prove product
contracts; they do not impose a business-test suite on authors of each mock.
There is no new authoring-time target, benchmark program, platform
qualification, publishing, or signing requirement. Linux and other non-local
environments remain explicitly unexecuted for this assignment.

## 7. Consequences and non-goals

Authors can serve a deliberately partial mock without maintaining a second
declaration of implementation progress. The product loses startup detection of
forgotten scaffolds, including those added by schema update. Invocation errors
make that tradeoff explicit without claiming that intentional failure implements
the eventual business behavior.

Do not add a replacement warning scanner, completion annotation, status command,
scenario runner, `mock test`, shared `require`, module loader, daemon, cache,
hot reload, external networking, persistence, or background worker. ADR-0050's
host and distribution work remains an independent assignment. This decision
removes authoring bookkeeping, not structural safety or runtime guarantees.
