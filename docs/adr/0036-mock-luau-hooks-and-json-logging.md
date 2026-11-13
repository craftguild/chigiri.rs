# ADR-0036: Mock Luau hooks, typed request state, JSON logs, and inference traffic visualization

- Status: Accepted
- Implementation: Implemented (local implementation accepted / designer review passed)
- Date: 2026-09-09
- Decision owners: Pythia maintainers
- Depends on: the ADR-0003/0016 Plugin and Endpoint contracts; ADR-0029 through
  ADR-0033 Mock execution, configuration, and inference; ADR-0034/0035
  formatting, CLI, and GraphiQL
- Amends: the Mock host API and HTTP rejection contract; project, bundle,
  reference, proposal, and management formats; `serve` output; and ADR-0033's
  progress display with a traffic view
- Scope: Lab01's `pythia-mock`, inference traffic display, and the minimal
  read-only common-context extension required to connect them
- Activation: a separate task after ADR-0034/0035 implementation and review.
  Do not add HK acceptance to work already in progress or to existing Passed
  results.
- Amended by: [ADR-0039](0039-request-atomic-mock-execution-and-soft-hard-limits.md)
  (2026-09-13, Accepted / Not started), which replaces root commit semantics,
  after-hook failure and reference points, the rejection-status set, and failure
  responses

2026-09-09 amendment: rejection specifies a numeric HTTP status, and `infer`
temporarily displays the actual request and response. This replaces the old
proposal's string rejection code and unspecified HTTP status. It does not add
persistent traffic history.

2026-09-13 amendment: the Implemented/Passed result in this ADR is historical
evidence for the old contract. The successor implementation prioritizes
ADR-0039, retains read-only authority for all hooks and a Rust-owned `next`,
moves `after` hooks before commit, and rolls back the entire request on an
exception or Hard-limit failure. Inference communication and TUI behavior remain
as specified here.

## 1. Background and decisions

Planned successor: [ADR-0051](0051-mock-contract-validation-without-completion-state.md)
removes only hook descriptor completion `state` and its bundle metadata at
implementation cutover. Typed `run` functions, initially disabled no-op hooks,
registration, request-local `ctx.state`, and `statePatch` remain supported.
Later request-atomic amendments remain authoritative for runtime failures.
Accepting this successor does not establish new implementation or test results.

Mock needs authentication, permission-based field rejection, request-scoped
markers, and observation of execution results. The common endpoint already has
request, operation, and field plugins, but Mock Luau lacked a corresponding
entry point. Rust owns execution progress; Luau performs only bounded decisions
and observations. The design adds no second GraphQL executor, Lua-owned
continuation, arbitrary plugin array, or persistent session store.

| Hook | Visible data | State update | Rejection |
| --- | --- | --- | --- |
| request.before | request, store, state | through a validated return value | allowed |
| operation.before | request, store, state | through a validated return value | allowed |
| field.before | request, store, state | not allowed | allowed |
| every after hook | store, state, and that phase's result | not allowed | not allowed |

`ctx.log` is available in all six slots. Hooks view the store through the
existing read-only `ctx.models` type. They receive no
create/update/delete/connect/disconnect operations, and Rust rejects writes at
runtime. After hooks are observation-only: they cannot change the response,
state, or store or revoke a successful result. Type declarations are only an
aid; neither `any` nor `pcall` bypasses Rust's permission, type, or budget
checks.

## 2. Project configuration and editing

### 2.1 Six fixed slots

Each slot has exactly one module; there is no registration order, glob, or
coordinate filter. `mock.luau` explicitly enables or disables each slot. All
hooks start disabled and state starts empty. `init` generates six editable
no-op modules without enabling them.
```text
mock/
├── mock.luau
├── generated/                 # Types, schema snapshot, and update metadata
├── hooks/
│   ├── request/{before,after}.luau
│   ├── operation/{before,after}.luau
│   └── field/{before,after}.luau
├── resolvers/                 # Existing query, mutation, and object resolvers
└── seeds/                     # Existing JSONL
```
Example configuration (the normative shape of the new format):
```lua
return {
    format = "pythia-mock-project/v2",
    schema = { sources = { "schema.graphql" } },
    server = { listen = "127.0.0.1:4000", allowed_origins = {} },
    hooks = {
        request = { before = true, after = false },
        operation = { before = false, after = false },
        field = { before = false, after = false },
    },
    state = {
        fields = {
            authenticated = { kind = "boolean" },
            user_id = { kind = "nullable", of = { kind = "string" } },
        },
        initial = { authenticated = false },
    },
}
```
The closed state-type vocabulary is `string`, `boolean`, `integer`, `number`,
`object`, `list`, and `nullable`. An object has `fields`; a list or nullable
type has `of`. Reject unknown properties and invalid combinations for every
kind. An integer is a finite safe integer (±(2^53−1)); a number is finite;
neither converts implicitly to or from a string.

State is a bounded, owned, JSON-like value. It cannot contain VM handles,
functions, cycles, or arbitrary Luau type strings. Charge it to the existing
depth, node, list, string, and total-byte limits, and bound both declarations
and values before expansion. Validate field names as generated identifiers so
canonicalization collisions and reserved names are rejected rather than
silently renamed.

Missing required initial values, mismatched types, and unknown fields are rejected before listening.
Only an omitted initial value for a nullable field is normalized to explicit
null; an empty table is not null. Each HTTP request receives an independent
initial state, so authentication and markers never carry over from a previous
request.

### 2.2 Update, inspection, and distribution

Generate type information from the declared state schema, IR, and shared host
API, never from configuration values. After changing configuration, regenerate
types, management metadata, and scaffolds with `update --config-dir …`, then
check readiness with `check --config-dir …`. A successful update does not imply
readiness; users must adapt their edited hook and resolver implementations to
the new types.

Apply ADR-0032's B/U/N and `.dist` rules to hooks as well. Update unedited
files; preserve edited files and place candidates in `.dist`. Toggling a slot
never deletes its user-owned file. Disabled invalid slots remain protected by
ownership, update, and recovery inventories, but are excluded from execution,
whole-project readiness type checking, and AI target selection. Re-enabling a
slot requires validation. Every slot remains subject to the existing read,
ownership, path, and resource boundaries.

New Luau and `.dist` go through ADR-0034 formatting → type/conformance checking → digest → save.
Do not reformat existing U files, comparison originals, backups, or recovery
material. Preserve conflict checks, directory replacement, and all-target
rollback. Export places validated configuration and enabled hooks in a portable
bundle that needs neither the original project nor external Luau files. It does
not dump runtime state or reformat hook bodies, and bundles neither execute nor
require disabled hooks. Keep `serve --config-dir` and `serve --config`
mutually exclusive and retain the pre-start seed/store consistency check.

### 2.3 Format switching

| Contract | New version | Reason for change |
| --- | --- | --- |
| project | v2 | hook registration/state type/initial value |
| portable bundle | v4 | evaluated configuration, enabled hooks, and state contract |
| host API | v3 | ctx/state/reject/log by phase |
| reference | v3 | hook permissions, types, return values, and examples |
| proposal | v3 | hook target and state/registration identity |
| update-state | v2 | hook B/U/N and generated identity |
| schema-snapshot | v2 | SDL and independent hook/state generation contract record |

Change each existing format identifier to the version in the table above, and
do not retain old readers or automatic compatibility conversion. The manual
migration procedure backs up the old project, runs a new `init`, moves the
user's sources and seeds, and then runs `check`. The absence of backward
compatibility must not be treated as permission to destroy old files or
recovery material.
The meaning of GraphQL ABI 6 and SchemaDigest is unchanged; state and hook
changes do not enter the SDL digest.
Rust 1.85 for the regular product and generated SDKs, and Rust 1.95 for Mock,
remain separate boundaries.

## 3. Luau surface and state

### 3.1 module and return value

Like a resolver, a hook module has `state = "ready" | "unimplemented"` and a
function named `run`. Readiness rejects a valid but unimplemented module; a
deliberate no-op scaffold may be marked ready. Generate distinct context and
return types for each slot, and derive execution, type declarations, and
text/JSON reference output from the same definition.

`request.before` and `operation.before` continue normally by returning `nil` or
`{ statePatch = … }`.
`field.before` and every `after` hook return only `nil`. Reject unknown return fields.
```lua
return {
    state = "ready",
    run = function(ctx)
        -- Real authorization decisions use ctx.request.headers and read-only ctx.models.
        if not ctx.state.authenticated then
            return { statePatch = { user_id = ctx.null } }
        end
        return nil
    end,
}
```
Before hooks reject with `ctx.reject({ code = 401, message = "Sign in required" })`.
`code` is an integer HTTP status; there is no parallel string-code API.
`message` is a bounded Unicode string, and omission selects a fixed rejection
message. `ctx.reject({ code = 200 })` still prevents `next` from running.
Decision rules for numerical validity, HTTP reflection, and multiple rejections follow Section 5.1. Do not copy raw VM/provider errors to GraphQL.
Do not inject reject in after. Imitating a rejection with an exception or invalid return does not change the result.

### 3.2 Meaning of statePatch

Rust owns state and passes read-only snapshots to Luau. Direct assignment is not
an update operation. `statePatch` replaces only top-level fields: omission
preserves a field, while explicit `ctx.null` clears a nullable field. Nested
objects are replaced as whole values rather than partially merged, and every
replacement must satisfy the declared type.

No patch becomes visible until every patch passes type, authority, and resource
validation; one invalid patch rejects the whole set. Pass the committed
`request.before` state to `operation.before`, then pass that same read-only
state to fields and resolvers. Do not create per-field branches, accept updates
from parallel fields, or share state through the store. After hooks receive only
the state finalized at their point in the lifecycle. If a before hook rejects
or fails, its uncommitted patches are never exposed.

## 4. Observation of request and phase

### 4.1 request snapshot

The before `ctx.request` is not Node.js's mutable `req`; it is the read-only,
owned snapshot below.

| Item | Meaning |
| --- | --- |
| method | actual HTTP method |
| host / port | numeric bind address and port of the real listener; never inferred from `Host` or `Forwarded` |
| url | received path and query, without an inferred scheme or host |
| body | UTF-8 string containing the same bounded raw JSON body consumed by the Endpoint; empty for GET |
| headers | array of `{ name, value }`; duplicates are preserved and lookup compares names case-insensitively |

Hooks see only requests that pass HTTP, size, and decode entry checks. Do not
call JSON produced by reserializing a parsed value the raw body.
Add only the read-only body reference needed by the common
`HttpRequestContext`; avoid a second decode, unbounded copying, or rereading.
Represent every byte of a non-UTF-8 header value as uppercase `\xHH`. In a
valid UTF-8 value, escape each backslash as `\\`. The reference must define a
reversible, unambiguous distinction between these forms.

HTTP-derived observation strings retain the entry limit of less than 1 MiB.
Charge additional copies to the request-wide depth-32, list-100,
node-100,000, and 16 MiB budgets. Preserve the 4 KiB limit for state, stored
values, and explicit logs. Do not automatically forward the raw request to
after hooks; pass information needed after authentication explicitly through
declared state. This change does not add hooks before HTTP parsing/decoding,
socket or body-stream control, or arbitrary response-header changes.

Operation context includes kind, name, and coerced variables. Field context
includes coordinate, response path, and coerced arguments. A field's parent is
the read-only stored value or DTO; relation and computed fields are not
pre-executed merely to populate it. Extend the common `FieldContext` parent view
only as much as necessary while retaining core runtime independence. This is
phase data, not a reason to add raw-request access to every after hook.

### 4.2 Scope of field

Field hooks cover every business SDL field, including root and nested fields,
stored and native values, automatic relations, and handwritten resolvers.
They exclude `__typename` and internal introspection fields, although request
and operation hooks still apply to operations containing them. The existing
executor first resolves fragments, aliases, `skip`, and `include`; hooks then
run once for each field invocation actually executed. Unselected fields invoke
no hook, and parallel fields have no fixed global order.

### 4.3 Store and reference point of results

The post-commit snapshot rules below describe the old contract. ADR-0039
Sections 3 through 5 replace them with request-candidate snapshots.

`request.before` and `operation.before` view the committed store snapshot at the
start of their phase. `field.before` uses the exact snapshot read by its
resolver; no different snapshot is acquired between hook and resolver.
`field.after` views the snapshot corresponding to the resolver result: after a
successful Mutation commit or after rollback on failure. `request.after` and
`operation.after` view the committed snapshot at the end of their phase. A
snapshot never changes during one hook invocation.

The `field.after` result is the resolver outcome, not the GraphQL subtree after
child selection completes. Pass only stored values or DTOs and safe error
summaries; never pre-execute or duplicate child resolvers. The common
`FieldError::safe_summary()` borrows only the public message and string code so
the budget can be checked before copying.
Preserve the fixed masking of internal errors, and do not add the source, error
ID, or optional extensions to this observation interface.
`operation.after` observes the operation's GraphQL result;
`request.after` observes the Plugin request-phase result. `request.after` is not
a transport-delivery completion hook; the access log separately observes the
final HTTP status. Do not raise output limits merely to observe large results.
Charge additional snapshots and conversions to the aggregate budget. An
observation-conversion failure is an after-hook failure and does not rewrite a
successful response.

## 5. Execution control/rejection/cancellation

### 5.1 Numerical rejects and HTTP responses

The following describes the old HTTP-override contract. ADR-0039 Section 7 supersedes it: retain rejection with status 200, roll back the entire request, return no partial data, and give internal failures precedence. The successor contract rejects other 2xx, 3xx, and bodyless statuses.

Every before-hook rejection code contributes to the final HTTP status. A
request or operation rejection is a request error. A field rejection follows
normal `FieldError`, path, and null-propagation semantics, but its requested
status applies to the whole HTTP response rather than to one field.
`code = 200` does not turn rejection into successful resolver output; it means
"reject the operation while returning its errors with HTTP 200." Set the
existing string-valued GraphQL `extensions.code` to `MOCK_REJECTED`, and put
the requested numeric value in `extensions.httpStatus`.
Lua users do not supply the string error code. Bodyless status exceptions are
defined below.

"Defined status" means the fixed set of final statuses allocated in the
[IANA HTTP Status Code Registry](https://www.iana.org/assignments/http-status-codes/)
as checked on 2026-09-09. Startup performs no network lookup.
The sets are 200-208/226, 300-305/307/308, 400-417/421-426/428/429/431/451, and 500-508/510/511.
Exclude Unassigned/Unused. 1xx is an intermediate response and is not used for final rejection.
Status 600, unassigned 499, 1xx, out-of-range values, fractional or non-finite
numbers, and strings passed through `any` all normalize to HTTP 500.
An invalid requested status still leaves the rejection sticky, records an
internal diagnostic, and returns the fixed message `Internal server error`.
Do not reproduce the user's message or source.

For 204, 205, or 304, preserve the requested status and send no body, as HTTP
requires.
Internal rejection and error observations remain available, but a GraphQL
client is not guaranteed to receive a JSON error body.
Do not forcibly attach JSON to the body-prohibited status, or leave invalid Content-Length/Transfer-Encoding.
This is a sending constraint according to [HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html#section-6.4.1).
This change adds no redirect, challenge, or other optional header settings;
specifying a status alone does not claim to simulate those complete exchanges.

If multiple fields reject with different codes, use the greatest normalized
numeric status.
For example, 200 and 403 produce 403; 401 and 403 produce 403; 401 and invalid 600 produce 500. Parallel completion order must not create last-writer-wins behavior.
The first reject is retained within the same hook and is not rewritten by the second reject after pcall.
Retain each GraphQL error/path and note in the reference that the final status and the specified value of each field may be different.

Mock retains the requested status in Rust-owned, request-local metadata and
applies it while assembling the final response. Resolvers cannot change HTTP
status by returning arbitrary extensions. Independent transport timeout,
serialization, output-limit, or similar failures retain their existing
priority and cannot be overwritten by a 200 rejection. After hooks cannot
rewrite status, and the access log records the actual final status rather than
the requested value.

This feature is an explicit status override for reproducing HTTP errors with Mock.
In particular, a 4xx response with partial data or a rejected request returned
with status 200 differs from the normal status selection described by the
[GraphQL over HTTP draft](https://graphql.github.io/graphql-over-http/draft/#sec-Status-Codes).
Do not change the default behavior or conformance of general endpoints, and do
not present Mock override examples as conformance tests. When no hook rejects,
preserve the existing media-negotiation and HTTP semantics.

### 5.2 Rust-owned progress and failure boundaries

The root-by-root commit behavior below, including continued success after an
`after` failure, describes the old contract. ADR-0039 supersedes it with a
single request-wide commit and whole-request failure.

The Rust plugin runs `before`, calls Rust's `next.run(...)` exactly once when
continuation is allowed, and passes its result to `after`. Use the existing
worker, owned-value, and cancellation paths; never await `next` while holding a
Lua VM, closure, or lease. Do not add Luau APIs for re-executing GraphQL,
calling `next` again, accessing the network or processes, or performing
unbounded I/O.

Rust records a `before` rejection as sticky. Catching it with `pcall` cannot
resume `next` or apply a state patch. A `before` exception, invalid return, or
resource overrun also stops progress. Request and operation hooks use the
existing GraphQL request-error path; field hooks use the existing `FieldError`,
path, and null-propagation path. Section 5.1 applies only the explicit rejection
status and does not allow arbitrary replacement of the whole response. If a
reached phase's `before` hook rejects, pass the rejected result to that phase's
`after` hook and do not invoke unreached inner phases. Cancellation and deadline
rules take precedence, so `after` is not guaranteed to run.

Treat an `after` exception, invalid return, write attempt, or resource overrun
as a safe diagnostic only; it does not change the original result, state, or
store. Do not retry a failed `after` indefinitely. Outer `after` hooks may
still observe the result independently while budget remains. An `after` hook
is neither `finally` nor a durable audit mechanism, and its failure does not
roll back a successful Mutation. Under this old contract, preserve the
existing transaction boundary for each Mutation root and do not roll back an
already committed root when a later root rejects.

The system may wait for a Luau decision, but never indefinitely or by blocking
a runtime thread on synchronization. Share the existing request-wide absolute
deadline, VM time and memory limits, worker-queue bound, `pcall` re-entry limit,
and cancellation/collection paths. Do not reset the overall request timeout for
each hook; run `after` only within the remaining budget. On cancellation,
unstarted hooks may be skipped, while active workers must be stopped and
collected. Dropping a `JoinHandle` alone is not collection. Disabled slots do
not create VM jobs. Measure state-initialization and field-hook amplification
without hiding their cost by loosening limits.

## 6. Hook log and HTTP access log

### 6.1 Common Output Contract

ADR-0039 Section 8 supplements this contract with diagnostics for later Soft
warnings, terminal transaction results, and safe stack and position data.

Provide `ctx.log(level, message, fields?)` for all hooks. Level is the closed
set `debug`, `info`, `warn`, and `error`.
The message and optional fields limit the type/depth/number/byte, and Rust serializes them as JSON.
Even if the source contains line breaks/ANSI, each record maintains one line. Luau's print and raw stdout writing will not be made public.
The common envelope has format, kind, timestamp, and Mock request ID, and the hook record adds phase, timing, level, and message.
Add coordinate/path to the field hook, and put user fields in a dedicated object. Do not allow overwriting of built-in fields.
Set the format to `pythia-mock-log/v1` and the kind to hook/access/log_loss.
`timestamp` is a signed integer count of UTC milliseconds since the Unix epoch;
`duration_ms` is a nonnegative value from a monotonic clock. Lua neither creates
nor changes request IDs. A `log_loss` record has `request_id: null`.

`serve` reserves stdout for JSON Lines. URLs, startup notices, and human-readable
diagnostics go to stderr. Keep the output contracts of `help`, `version`,
`reference`, and `export` unchanged, and never mix progress or color escapes
into JSONL. Section 7.2 defines `infer` display output separately. Logging
requires no AI or external service.

### 6.2 HTTP access log

Access logging is active whether or not hooks are registered. Observe requests
outside the server's routing and admission boundary, including `/graphql`,
GraphiQL and assets, OPTIONS, 404s, and CORS, size, or decode rejections. Use one
Mock request ID to correlate hook and access logs. If it differs from Pythia's
`RequestIdentity`, do not claim they are the same identifier. A broken
connection before the HTTP parser creates a request has no access record;
never invent a method or path.

The minimum record is timestamp, request_id, method, path, status, duration_ms, and outcome.
`path` excludes the query string, and `outcome` is the closed set `completed` or
`cancelled`. If cancellation prevents response construction, `status` is null.
`completed` means only that server-side response processing ended, not that the
peer received it. Enqueue exactly one terminal record per request; do not encode
cancellation as a fictional status or a duplicate record.

Do not include raw document, body, query string, headers/token, state, or result body in automatic logs.
Because a path itself may contain a secret, do not describe access logging as
complete anonymization. Values explicitly passed by the user to `ctx.log` are
intended for output; the implementation cannot guarantee detection of every
secret in arbitrary messages or fields. Reference and usage examples warn
against copying authorization, cookie, or body values directly into fields.
Keep automatic VM/provider diagnostics redacted and distinct from explicit
user logs.

### 6.3 Log loss and outages

Hook and access records use bounded queues with separate reserved capacity, so
high hook-log volume cannot consume every access-log slot. A dedicated writer
produces output; GraphQL processing and the VM never wait synchronously on
stdout. If the queue is full, the destination stalls, or a `BrokenPipe` occurs,
drop the log and continue the response. Log loss alone must not close readiness
or stop the server.

Maintain per-kind drop counters and rate-limited diagnostics. When output is
possible, report loss through `log_loss` records and the shutdown summary.
Provide a best-effort stderr diagnostic when stdout is broken, but do not
promise delivery when both streams are blocked.
Failure to report log loss must not recursively enqueue more notifications.
Shutdown never waits indefinitely to flush every item. Writer output, flush,
stop, and recovery all share the common shutdown deadline. Do not detach a
blocked writer thread after timeout and call it stopped.

To exercise the authentication example from a browser, allow `Authorization`
in CORS preflight for explicitly permitted origins. Preserve exact origin
matching, GraphiQL's own numeric origin, and the existing credentials policy.
Do not add wildcard origins, unconditional reflection of arbitrary headers, or
automatic injection of authentication values.

### 6.4 Writer implementation boundaries

The Rust serializer limits each record, including its newline, to 16 KiB.
Reserve 64 queue entries each for hook and access records and eight for stderr
notices. Attribute full queues, serialization overruns, and unconfirmed
delivery to separate cumulative hook, access, and notice drop counts. Emit
notices at most once per second plus a best-effort shutdown summary, and do not
count a notice's own failure recursively. Enqueueing is not successful
delivery; only a confirmed writer `write` and `flush` counts as delivered.

Because a nonblocking flag alone cannot bound completion of a file write, a
private same-binary worker process owns stdout and stderr. The parent
coordinator handles only bounded queues and dedicated nonblocking pipes. Each
sink permits one unacknowledged frame. A frame has a four-byte length prefix;
the child accepts at most 16 KiB and writes it synchronously to its stdio sink.
The child uses a private acknowledgement pipe in place of stderr. The notice
sink is a duplicate of the parent's stderr and never mixes in raw child
diagnostics. Logging workers share the inference worker's process-group owner,
but logging startup does not inherit the environment or call any AI backend.

If a write fails before sending any byte, drop that record and allow a later
record. If a partial write or flush failure makes delivery uncertain, close the
sink and never append JSON after an unknown partial frame. Forced shutdown may
still leave the last frame incomplete. Count unacknowledged frames
conservatively as drops. When both output channels are unavailable, exact
comparison with received frames and delivery of a summary are not guaranteed.
Preserve one-line boundaries for complete JSON records and JSON-escape user
strings. Share the existing five-second deadline, measured from the signal,
across HTTP, VM, and coordinator shutdown. The signal also cancels pending
body-read futures.
If shutdown produces `503` with `Connection: close`, record the actual status
in the access log.
Drain connections whose clients do not read responses only until 500 ms before
the deadline; exceeding that point is a nonzero HTTP-drain failure. Route
shutdown diagnostics from a running server through the bounded writer; `main`
must not retry synchronously on stderr. Close new submissions 300 ms before the
deadline and finitely discard the remaining queue, ordering closure and
`try_send` under a short lock. Attempt summary delivery no later than 150 ms
before the deadline, then use the remaining time to kill and reap both children,
confirm that the process group is gone, and join the coordinator. A kill request
or elapsed deadline alone is not success; an OS cleanup failure that can be
resolved only by the final wait owner remains an explicit failure.
Mock directly uses the existing `http-body` 1.0.1 dependency to observe body
completion and drop. Do not change shared-crate dependencies or MSRV.

### 6.5 Subscription log v4 successor

At the coordinated ADR-0054 cutover,
[ADR-0054 Section 9.6](0054-graphql-subscriptions-and-bounded-stream-delivery.md#runtime-log-v4-records-and-ownership)
adds four closed subscription record kinds and changes the format to
`chigiri-mock-log/v4`. Its exact fields, once points, correlation, redaction,
and internal ownership contract are normative. This is not a new receipt,
public observer, or log-ingestion interface. Existing hook, access, diagnostic,
and fresh-request `request_terminal` semantics remain unchanged.

One new 64-entry subscription queue shares the existing `QUEUE_RECORDS` bound;
all four kinds use it on stdout. Hook/access reserved queues and the eight-entry
stderr notice queue remain isolated. Fair stdout rotation covers all three
classes. Loss adds one saturating subscription counter, so `log_loss.dropped`
contains exactly hook, access, subscription, and notice. Record bytes, output
confirmation, isolated processes, nonblocking producers, and the original
shutdown deadline remain as specified above. This successor does not relabel
historical v1/v3 validation as v4 acceptance.

## 7. Reference and AI inference

### 7.1 Hook candidates and common references

The host API definition records each of the six slots' authority, return value,
`statePatch`, rejection and logging APIs, types, limits, and example. Derive
Rust conformance checks, Luau declarations, and text and JSON references from
that definition; no second handwritten API specification is authoritative. A
schema-only reference describes a generic state contract, while project-derived
types and inference context include the actual state type.
Reference display is not project readiness, and the mere presence of a disabled
invalid hook does not make the whole project unstartable.

Reference v3 gives each slot a fixed path, validated registration, and links to
its context, return, and module types. Common and schema-only input represents
state as an unset generic map; project and bundle input uses a closed type with
a validated declaration. The reference excludes initial values and hook source.
Displaying configuration-derived types and registrations does not claim
readiness.

Snapshot value types, log and rejection arguments, and arity share the typed
host definition used by Luau declarations and the type graph. The runtime
definition also supplies the allocated status set, body-prohibited statuses,
invalid-status fallback to 500, and log format. Repeated state type names do not
avoid the independent 16 MiB limits on type-graph construction and text/JSON
rendering.

`infer` and `apply` add a valid hook only to an explicitly selected target. As
with resolvers and seeds, every item starts unselected.
A hook's inference context includes slot, phase permissions, request/result/model types, state contract, and current source and reference.
`mock.luau`, state type/initial value, hook registration, SDL, and generation management information are read-only contexts for inference and are not targeted for writing by AI.
Do not automatically enable invalid hooks, replace unselected hooks, or collect or send requests/tokens/stores during execution.
User source and configuration can enter the transmitted context, so retain the
existing pre-send confirmation and secret-handling precautions.

Mock inference owns GraphQL, phase, and partial-candidate validation; the Codex
adapter owns only process and communication handling. The generic backend trait
does not learn hook or `.dist` semantics, and this change requires neither a
new provider nor a new real-AI dependency. After ADR-0034 formats a candidate,
validate its slot-specific type, module, authority, and association set before
saving it in a proposal.
Include typed state changes and valid slot set in proposal identity, and check drift after saving before approval/placement.

Alongside configuration source and generation metadata, render evaluated state declarations, initial values, and registrations into a logical context item named `configuration.json`.
All four materials are read-only context. Before transmission, the inventory
records their byte counts and digests. They grant no arbitrary file-read scope
and add no write targets.
Proposal v3 has the same ConfigStamp (registration and regular state contract digest) as management information, making it independent from SchemaDigest.
Owned sources containing invalid hooks are included in the input verification of the adoption transaction, but only valid slots are added to the active body sent to the AI.
Resolvers and hooks together allow at most eight modules per batch; a seed batch
contains at most 64 models. Including the six hook slots, there are at most 326
targets. Thirty-four batches with an initial request and up to two revisions
produce at most 102 calls.
Derive the upper bound from structure and reflect it in the manifest's usage
and context counts. Use the default 600-second deadline or an explicitly
specified shared deadline, and do not relax byte limits for candidates,
prompts, or proposals.
Even when each material meets its own limit, reject before transmission if the
actual rendering, including read-only material and JSON escaping, exceeds the
prompt limit. Independent maxima are not guaranteed to fit simultaneously, and
material is never omitted silently. If shared context alone is too large,
splitting selected targets cannot help.
Do not reuse successful type checks in old contexts and follow existing conflict/retest/reauthorization agreements.

Retain the full-text TUI diff, explicit accept/reject choice, final approval,
installation of the exact approved bytes, non-TTY save-only behavior, and the
rule that AI adoption does not advance B to U.
For a proposal combining hooks with resolvers or seeds, do not regenerate unrelated valid candidates. Keep partial conformance separate from overall readiness.
Run both structural checks for types, formats, ownership, and capacity and
semantic authentication and state checks over real HTTP.
`check` validates hook syntax, types, and modules; it does not fabricate a request and claim the user's hook executed.

### 7.2 Temporary display of infer communication

In addition to coarse `infer` progress, users can see what is sent, what is
returned, and why a revision is requested. The display is enabled by default.
It adds no dedicated save, playback, or search database and no external
telemetry. It applies to existing resolver and seed inference as well as hooks;
`apply` never calls the AI. Existing proposal and recovery retention rules are
unchanged, and the new traffic history is not copied into those materials.

| Display | Content and certainty |
| --- | --- |
| Scheduled to be sent | Actual full prompt and expected output schema for each batch and attempt; never replace them with only a summary |
| Transmission completed/incomplete | Distinguish handing data to the adapter from writing it to stdin or another transport; completion confirms transmission, not successful inference |
| Backend operation | Observable common events such as start, wait, process, and stop; never invent unobservable work |
| Received message | Provider-published user-visible message body, displayed when a retrieved chunk or message is complete |
| Received response | Actual candidate response before formatting, protocol validation, or type checking, clearly labeled `unverified` |
| Correction request | The fixed reason for the inspection and the correction prompt that will actually be sent next. Do not display explanations left to the backend as inspection facts |
| Completed/Failed/Cancelled | Existing classification derived from the terminal event, exit status, and response matching; display alone is not evidence of adoption or success |

Prompt display is limited to content that Lab01 passes to the adapter. It does
not claim to expose private instructions or context added inside the backend.
Sent and received text, including candidate source and seeds, appears on a
dedicated user-visible traffic surface. This is a narrow observability exception
to ADR-0033's normal no-source-diagnostics rule, not a raw process dump.
Do not display raw stderr, environment or authentication files, argv dumps,
tokens, arbitrary tool output, private reasoning, or internal thoughts. Raw
Codex event types do not enter the TUI; the adapter converts only user-visible
messages and safe activity classifications into common events. Accountability
comes from actual input, public responses, and validation results, not a claim
to expose a model's internal reasoning.
If a prompt or source contains a secret, the traffic view may show it. Complete
automatic redaction is not guaranteed, so users must review transmission
carefully. Render control characters, ANSI/OSC sequences, and forged terminal
input through the existing reversible sanitizer; traffic text is never
interpreted as a UI command or approval.

In TTY mode, a communication pane accompanies the ADR-0035 ratatui progress screen while preserving the existing batch, attempt, remaining-time, and cancellation display.
Retain only bounded recent history and allow scrolling within it. Do not offer a
full-history replay or make inference wait for rendering. In non-TTY mode,
write undecorated text with direction and stage labels to stderr without
polluting command results on stdout.
The output purpose is different from serve's JSONL/access log, and the HTTP request ID is not used as the inference run ID.
Correlate events by run, batch, attempt, direction, and message ID. Distinguish
incremental chunks from full-text replacements without repeatedly appending the
whole text. A backend without chunks displays its response only when reception
completes; do not synthesize a token stream.

A bounded observation port sits at the common backend/control boundary. Mock
owns prompts and validation; the adapter owns actual transmission, reception,
and activity classification. The sink is optional and independent of the final
result protocol, and producers never write directly to the terminal. Bound
queue, event, history, and rendering bytes and item counts. Report dropped or
omitted events by count instead of retaining unbounded copies of every prompt
and response.
Slow rendering, closed non-TTY stderr, or a full display queue may drop only communication display; backend pipes must continue draining.
Dropping display events never omits the protocol-validation stream and never
weakens exit, terminal-event, final-file, or byte-limit checks. Preserve job
collection and terminal restoration on failure or cancellation, and distinguish
loss of the display from loss of the operation's terminal result.

Reading the traffic pane does not replace review of the candidate's complete
diff or explicit acceptance and final approval. Displaying transmitted input
does not approve later output. Formatted candidate bytes still receive the
existing full-text display and explicit approval. Required verification uses a
fake backend or executable with real PTY and pipe tests; it adds no real Codex
calls or charges.

### 7.3 Common observation and delivery boundaries

The observation queue holds at most 64 status or text items, with at most 8 KiB
of text per item. Split UTF-8 only at character boundaries; retain invalid UTF-8
responses as byte strings. For each update, record total bytes, offset, and
update number so a delivery gap cannot be mistaken for a continuous range of
the original text. Assign run numbers independently within the process;
batch and attempt belong to the request scope. Message IDs are at most 128
bytes. Full queues, disconnections, and item-limit excess increment cumulative
record and body-byte loss counts. Loss notifications do not re-enter the same
observation queue.

The engine emits actual prompts and schemas, adapter handoff, unvalidated
responses, fixed validation reasons, and run classifications. The Codex adapter
emits the complete public agent-message update as `Replace`; it does not
misrepresent it as a genuine delta. Group chunks only when they share a message
and update. `Replace` supersedes the previous complete text. The process layer
records bytes actually written to stdin; failure before startup is an
incomplete transmission of zero bytes. Public events and text display never
substitute for terminal, exit-status, and final-file agreement, and callback
delivery cannot turn failure into success.

TTY is limited to 256 KiB of display text and 64 recent history items, and distinguishes between tail tracking and past browsing.
Cache each retained item's wrapped-line count for the current terminal width,
and update it only when text is added, replaced, or deleted or the width changes.
Scrolling and height changes scan only the first visible item and display range,
not all retained text. The cache stores line counts without duplicating body
text or changing body, item-count, or screen-byte limits.
Non-TTY output renders line breaks visibly and labels each line with direction,
stage, run, batch, attempt, message, update number, and byte range. Multi-line
items also receive a part number. Its output queue holds 64 rows of at most
16 KiB each. Reuse the JSONL writer's independent single-line frame and
acknowledgement protocol and process-group ownership, while keeping body queues
and loss classifications separate. Even for regular files, the parent performs
no synchronous write, and a blocked sink or `BrokenPipe` does not stop the
inference job. Emit loss notices at most once per second plus one final
best-effort notice; a notice failure does not trigger the next notice.
At shutdown, allow 150 ms for ordinary delivery and up to 100 ms for the final
notice, then confirm child collection and process-group disappearance within a
separate five-second cleanup deadline.
Do not add another line to a sink that discarded an intermediate line. If delivery is impossible, the termination diagnosis is also delegated to the same bounded port, and synchronization is not retried from main.
Cancellation/restoration due to an abnormality in the actual TTY terminal maintains the existing contract and distinguishes it from loss only in the communication field.

`infer` and `apply` do not write the command summary synchronously to stdout,
even after restoring the terminal's original flags.
The summary and undelegated final diagnostics are passed to an independent stdio writer, which checks each 16 KiB frame within 250 ms.
Set a common 5 second absolute deadline for delivery and collection for both sinks, leaving the last 500 ms for collection.
Failure to deliver the summary preserves an existing primary cause; without another cause, it becomes an environment failure. Saved proposals remain available even when delivery fails.
If diagnostics cannot be delivered, `main` does not retry synchronously. If
writer recovery is uncertain, exit 2 rather than reporting successful
cancellation.

## 8. Acceptance and implementation order

2026-09-10: The integrated source completed the common 85 commands,
measurements, independent Reviews A/B, correction, and rereview. See the
integrated validation record for each ID
and source trace. The designer review on the same day also passed with zero
findings at every severity. Human UI/IME behavior and remote CI remain
unconfirmed.

### 8.1 Gates

| Gate | Range | Local judgment |
| --- | --- | --- |
| HK-A | HK-01–06: configuration, types, ownership, update, and distribution | Passed |
| HK-B | HK-07–12 and HK-31–32: Plugin, state, rejection, snapshots, commit, and numeric HTTP status | Passed |
| HK-C | HK-13–18: bounded execution, logs, access logging, redaction, and CORS | Passed |
| HK-D | HK-19–24 and HK-33–36: reference, inference, approval, second-domain HTTP, and traffic display | Passed |
| HK-E | HK-25–30: integrated validation, measurement, regression, review, and handoff | Passed |

### 8.2 Closed acceptance inventory

| ID | Required assertion |
| --- | --- |
| HK-01 | `init` creates six fixed slots, all disabled, with empty state; Clap grammar is exact and unknown registrations are rejected. |
| HK-02 | Every state kind has a positive case; missing required initial values, invalid types, unknown fields, cycles, non-finite values, and capacity excess are rejected; requests remain independent. |
| HK-03 | Declarations, type checking, execution, and reference output agree on authority; store/state writes and after-hook rejection are denied even through `any`. |
| HK-04 | `statePatch` preserves omission, distinguishes null, replaces top-level fields, forbids nested partial merge, applies nothing on failure, and propagates in order. |
| HK-05 | B/U/N and `.dist`, repeated update, disable/re-enable, concurrent edit and recovery, and preservation of user-owned and disabled hooks. |
| HK-06 | Closed validation of all seven new formats, rejection of old formats, non-destructive migration, mutually exclusive serve modes, export, and identical hook execution without the original project. |
| HK-07 | Rust owns one execution of next, normal before/after order, rejection observation of reached phases, non-execution of unreached phases. |
| HK-08 | Raw body match, GET, duplicate header, method/URL, no host impersonation, correct phase information in variables/args/parent. |
| HK-09 | Covers root, nested, native, stored, relation, and handwritten fields; verifies aliases, fragments, directives, and paths; excludes `__typename` and introspection internals. |
| HK-10 | Request/operation rejection and field null propagation; catching rejection or resource failure with `pcall` cannot resume `next` or apply a patch. |
| HK-11 | Consistency of store snapshot and resolver results for each phase. Do not double-execute child selection in field.after and do not falsify the overall result. |
| HK-12 | Under the historical contract, after-hook failure or an invalid return leaves a successful result, state, and commit unchanged; earlier Mutation-root commits remain and the failing root rolls back. |
| HK-13 | Debug and release stop infinite loops, excessive allocation, and `pcall` re-entry in every slot; the same server recovers with numbering and both models consistent. |
| HK-14 | Common absolute deadline/cancellation/queue full/stop. Omit after, deactivate VM in invalid slot, collect all workers/writers. |
| HK-15 | Log type/capacity/JSON escape for all slots, JSON parse for all lines of stdout, startup notice is stderr, common ID correlation. |
| HK-16 | Enqueue exactly one access record for GraphQL, GraphiQL, assets, OPTIONS, 404, entry rejection, and cancellation; verify status, nullability, and duration accuracy. |
| HK-17 | Access reservation during hook queue saturation, slow/closed stdout, drop counting/diagnostics, response continuation/readiness unchanged, bounded shutdown. |
| HK-18 | body/query/header/token/state/result unexposed to automatic logging/diagnostics. Explicit log responsibility explanation, Authorization preflight and Origin denial. |
| HK-19 | Text/JSON reference derived from the same definition, available APIs/types/examples for each phase, distinction between schema-only and real state. |
| HK-20 | Select only valid hooks explicitly, provide read-only configuration and context, reject unselected, disabled, or configuration-rewrite candidates, and collect no runtime secrets. |
| HK-21 | Detect errors in hook candidate format/type/module/authority with fake backend and return to corrected loop. Separate partial fitness and global readiness. |
| HK-22 | Proposal v3 detects state or registration drift, modification of a saved candidate, and conflicts with originals; stale validation or approval cannot authorize installation. |
| HK-23 | Real PTY transmission confirmation/full text approval/cancellation, non-TTY storage, placement of the same candidate bytes, AI adoption U/static B unchanged and rollback. |
| HK-24 | In the existing two domains/renames, auth→state→field rejection→after observed actual HTTP. Both project/export, offline after adoption, non-leakage during request. |
| HK-25 | Common validation; regular 1.85, Mock 1.95, and stable checks; ABI 6 and SchemaDigest; all-SDK exactness and quality; dependency boundaries |
| HK-26 | Preserve ADR-0030 through ADR-0035 regressions for types, nulls, relations, seed startup validation, seven operations, formatting, approval, GraphiQL, and the VM. |
| HK-27 | Compare startup, response, field count, queues, and observed RSS for the same operation with hooks disabled and enabled; also record binary size and incremental rebuild cost. |
| HK-28 | Verify the 400-line handwritten limit, generated-output exemption through ownership checks, fmt, Clippy, rustdoc, diffs, and removal of old formats and duplicate API paths. |
| HK-29 | Independent read-only Review A/B, correction/re-review of all issues, separation of backlogs and local/remote CI/designer approvals. |
| HK-30 | All IDs are associated with individual test/assertion/command/result, and usage/migration/log loss/recovery procedures, untracked and remaining restrictions are recorded. |
| HK-31 | Actual HTTP inspection of 401/403/200 in request/operation/field. Illegal 600/499/1xx/decimal/non-finite value/any string is 500, 204/205/304 has no body. |
| HK-32 | Reversing parallel-field completion order preserves the maximum status; `pcall` cannot change the first rejection, extensions cannot spoof it, independent transport failure takes precedence, and access logs record the actual status. |
| HK-33 | The display of the actual prompt/schema/modification request/reception response matches for all hook/resolver/seed infers. Distinction between batch/attempt, incomplete submission, and unverified/verified. |
| HK-34 | Fake-backend public messages, chunks, and completion events; midstream failure, invalid response, and final-validation mismatch; no transcription of raw stderr, authentication data, or internal reasoning; control-character sanitization. |
| HK-35 | Verify the real-PTY communication area, non-TTY stderr and clean stdout, bounded history/drop notices, and pipe drain/deadline/cancellation/recovery with slow or closed sinks. |
| HK-36 | Traffic display does not replace the full-text diff, send confirmation, acceptance, or final approval; typeahead does not carry over; no traffic-history file is added; existing proposal and recovery material remains. |

### 8.3 Development and review boundary

Build focused verification in this order: configuration/type/update →
plugin/state/snapshot/HTTP status → logging →
inference/approval/communication display.
Run focused target tests during implementation and consolidate expensive shared validation in the final source.
Reuse existing HTTP/VM/PTY/fake backend/transaction helpers and avoid creating another testing framework.
Review A independently checks hook authority/state/GraphQL/HTTP status/commit, and Review B independently checks resources/cancellation/log/ownership/inference approval/communication display.
Additional actual Codex billing/transmission, browser E2E infrastructure, publisher/SSH, and application modifications are not required gates.
Report unexecuted Linux/macOS remote CI, arbitrary manual browser confirmation, and designer approval separately from local Passed.

## 9. Consequences and non-goals

Authentication, request markers, and field rejections can be written using small typed hooks. There is no need to change SDL for HTTP reasons.
The tradeoffs are the need to update generated configuration/state types, the
execution cost of hooks on every field, and the risk of putting secrets into
explicit logs. Numeric rejection can reproduce an HTTP status directly, but it
does not always match GraphQL's standard status selection.
Inference communication is used for on-the-spot explanations, and is not positioned as audit history or visualization of AI internal reasoning.
The design provides no unrestricted HTTP-body or argument rewriting in
`before`, result replacement in `after`, or store mutation from hooks.
It does not extend to production authentication infrastructure, persistent
audit delivery, request-wide transactions for every Mutation, or handwritten
Rust adapters for each schema. Implementation had not begun when this ADR was
added. On 2026-09-10, normal hook execution, migration of all seven formats,
TUI integration, and local acceptance of every gate were completed. The
integrated validation record maps the
individual results to the final source. Designer review also passed. Human
UI/IME checks and remote CI remain separately reported as unverified.
Do not reuse ADR-0034/0035's existing Passed results as acceptance evidence for
these hooks.
