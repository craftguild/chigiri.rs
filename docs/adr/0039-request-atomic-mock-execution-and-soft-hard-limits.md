# ADR-0039: Request-atomic Mock execution and Soft/Hard limits

- Status: Accepted
- Implementation: Implemented. RA-01 through RA-18 and Gates RA-0 through RA-D completed local implementation acceptance; the final designer implementation review passed on 2026-09-14.
- Date: 2026-09-13
- Decision owners: Pythia maintainers
- Approval: complete request rollback, read-only `after` hooks before commit, Soft/Hard limits, explicit rejection, separation of HTTP status, and stderr diagnostics were approved by the decision owner
- Design detail: The decision owner specified initial numeric values, concurrency controls, response formats, and input requirements so the contract could be implemented without additional interpretation.
- Amends: ADR-0029, ADR-0030, ADR-0031, ADR-0036, and ADR-0038 with request-level commit boundaries, complete failure handling, atomic rather than partial success, and shared execution-budget semantics.
- Replaces: the rejected 3:1 after-observation budget proposal
- Preserves: ADR-0038 model and relation constraints, the existing GraphQL executor, generated ABI 6, normal Rust 1.85 compatibility, and Mock Rust 1.95 compatibility.
- Goal: Subsequent implementation goals

At the time of design approval, implementation and verification were not complete, and candidates at the start of the transition had root commits and observation reservations.
The common 102-command verification, same-release-binary measurements, and
independent Reviews A and B with correction and rereview are now complete. The
verification record is authoritative
for final-source evidence, individual assertions, guarantee limits, and
unexecuted work. Existing ADR-0038 results, tests, and recovery materials are
retained; only the superseded contracts change. This ADR does not authorize
changes to external applications.

## 1. Background and decision

When resource exhaustion skips an `after` hook, a successful response cannot
prove that all user-configured processing ran. The 3:1 reservation isolated
observation failures within the aggregate limit but reduced normal processing
capacity by the reserved amount and still allowed partial observation. Root-
level commits also left earlier Mutation updates in place when a later root or
`after` hook failed.

The Mock contract becomes: "Processing and response construction reflect only a request that completed successfully."
Crossing a Soft limit emits a warning and processing continues. A Hard-limit
failure, exception, explicit rejection, or cancellation discards candidate
state and returns no success data.
HTTP status represents the execution result; it is not a commit condition.
`reject({code=200})` is still Rejected. This is an explicit Mock execution
policy and does not change ordinary Pythia consumers' GraphQL partial-success or
status-selection semantics.

## 2. Guaranteed unit and state

The unit is one GraphQL operation selected by the current HTTP entry point,
together with its request, operation, and field hooks. Multiple Mutation roots
belong to the same request. Batch execution, Subscription, defer/stream, and
recursive GraphQL execution are not added.

Rust owns the request lifecycle. Its conceptual state is shown below; these
names do not prescribe public type names.

```text
Running -> Prepared -> Committed
    |          |
    +----------+--> Rejected / Failed / Cancelled -> Discarded
```

- `Rejected` means an explicit refusal by the user; `Failed` means a type,
  processing, or resource failure. Cancellation cannot turn back into success.
- Values returned by parallel fields or `pcall` never overwrite failure state.
  HTTP status and GraphQL extensions do not determine success.
- A pure Luau exception during request execution remains a failure after
  `pcall` catches it. A valid rejection keeps its classification through
  control interruption and cleanup cancellation. Pre-start configuration
  evaluation is not mixed with request execution results.
- Any GraphQL error makes the request unsuccessful, including errors on
  nullable fields.
- A valid null, empty list, or zero-result search permitted by SDL is a
  successful value, not a failure.
- Query uses the same all-or-nothing decision over a read-only snapshot and
  never mutates the store.

## 3 - Candidate status and parallel control

Each Mutation request creates one candidate state and applies every root's
changes to it.
Model records, internal row and identity management, relation edges and ordinal
indexes, and capacity accounting all belong to the same exchange unit.
Request state belongs only to its request and is never exposed to other
requests. Failed request state is not carried forward. The ULID random source
need not be rewound, and the same ULID is not guaranteed after failure.

The initial implementation serializes Mutation requests with one asynchronous,
request-scoped write permit. It must not return to acquiring a permit for each
root.
After acquiring the permit, the implementation creates candidate state from
the latest committed state and owns it from `request.before` through commit or
discard.
Admission wait is bounded by the same absolute deadline, cancellation token,
and existing finite admission capacity; waiting tasks cannot grow without
bound.
No HTTP or plugin `next` call is awaited while holding a mutex guard or Luau VM,
and the worker never reacquires the same permit.

Other requests cannot refer to candidates. Query keeps a single committed snapshot taken at the start of the entire phase.
When commit races with Query startup, Query receives either the complete old
generation or the complete new generation, never a mixture.
It prohibits routes where two Mutations update and overwrite from an old
snapshot, and routes where a failed request rolls back another request's
commit.
This ADR adds no automatic retry, script rerun, optimistic transaction engine,
or persistent database.

The Mutation root in the same request follows the existing GraphQL execution order, and the subsequent root can refer to the candidate update of the previous root.
Child selections and field hooks for a root read its corresponding candidate
snapshot and never switch implicitly to another generation.
While field parallelism can be maintained, the common budget, failure status and authority for candidates is managed by Rust.

## 4. Hook and commit order

```text
request admission -> operation selection -> request owner creation
  -> Mutation permit/candidate store (or Query read-only snapshot)
  -> request.before -> operation.before
  -> field.before -> resolver -> field.after (for every selected field)
  -> operation.after -> request.after
  -> final consistency and response-encoding exposure limits
  -> all jobs complete -> failure/cancellation/deadline summary -> commit decision
  -> HTTP response is ready to send
```

Only `request.before` and `operation.before` may update state through a
validated return value. `field.before` cannot update state but may reject.
Store references in every hook, and state and results in every `after` hook,
are read-only. Every `after` hook observes pre-commit state; it is neither a
success notification nor a guaranteed `finally` block.

| Phase | Store/result reference point |
| --- | --- |
| request/operation.before | The request's initial snapshot or candidate. |
| field.before | The same candidate snapshot read by the target resolver. |
| field.after | The target resolver's candidate result, without executing child selections early or twice. |
| operation/request.after | Candidate state after every field succeeds and the successful response draft for that phase. |

An otherwise successful request still fails if an entered hook exhausts its
budget.
Exceptions, invalid returns, write attempts, and Hard-limit exhaustion in an
`after` hook fail the entire request. After failure, unstarted processing stops
and unfinished jobs are cancelled and recovered; remaining `after` hooks are
not forced to run as cleanup. Omissions caused by failure before entry are
recorded in terminal diagnostics rather than hidden as success. Disabled hooks
and unselected fields remain unexecuted.

## 5. Commit fence and guarantee limits

Committed state is never exchanged inside a root resolver or `field.after`;
there is exactly one exchange point per request.
The current cancellation/exchange ordering is preserved, with one fence that
prevents cancellation from being missed between final validation and exchange.
The synchronous commit section contains no `await`, log I/O, response encoding,
user code, or large allocation that could fail.
Failure state, cancellation, shutdown, the absolute deadline, and candidate
consistency all feed the single final decision point. A closed worker commit-
response channel is not assumed to mean "uncommitted"; the Rust-owned terminal
state is checked directly.

Successful responses are completed in a bounded buffer before commit. Encoding
failure or an oversized response discards every candidate. The final `after`
hook observes a response draft before encoding and therefore is not a delivery-
success notification. A fixed small error envelope provides a bounded failure
path independent of the exhausted operation budget.
This fixed envelope is not a second budget for extending user processing. Failure to produce it is not diagnosed or retried recursively.

Cancellation before the exchange point discards the entire candidate; an
exchange that wins the ordering is committed. Socket disconnect or transmission
failure after commit does not roll state back. Already emitted stderr or
explicit log records are not retracted, and their presence is never proof of
commit. No response or recovery guarantee is made for `panic = abort`, an OOM
kill, or forced process termination.

## 6. Soft/Hard limits

The 3:1 reservation, `after`-specific subledgers, and the path that treated
resource failures as diagnostics while continuing success are removed.
`before`, resolver, `after`, codec, and candidate operations share one
cumulative request ledger; remaining capacity is not reset per phase or root.
Without raising existing Hard limits, their 75% point becomes the default Soft
warning threshold. No user setting or new CLI flag is added in this change.

| Request cumulative resource | Soft | Hard |
| --- | ---: | ---: |
| Work | 750,000 | 1,000,000 |
| Value nodes | 75,000 | 100,000 |
| Logical bytes | 12 MiB | 16 MiB |
| Regex evaluations | 3,072 | 4,096 |
| Regex input bytes | 768 KiB | 1 MiB |
| Shared elapsed time, including waits, hooks, and encoding | 3,750 ms | 5,000 ms |

The first crossing of Soft emits a warning and continues. Hard itself is
allowed; an operation that would exceed Hard is rejected before reservation or
execution. Time fails at or beyond the absolute deadline rather than resetting
a relative timeout for each phase.
If one charge crosses both Soft and Hard thresholds, Hard failure takes
precedence and all information is folded into one terminal diagnostic.
Soft does not set sticky failure or alter results, state, or execution order. A
warning appears at most once per request and limit name. Work is a policy cost,
not an instruction count or measured latency; byte and node budgets are not
process-RSS guarantees.

Existing Hard guards remain: 500 ms and 16 MiB per VM execution, 256 host
calls, 16 simultaneous `pcall` re-entries, a queue of 16, and a five-second
shutdown deadline. Local VM memory, host-call, and `pcall` re-entry failures
remain typed Hard causes separate from the six shared resources. HTTP reports
`MOCK_EXECUTION_LIMIT_EXCEEDED` with a fixed limit name such as
`vmMemoryBytes`, `hostCalls`, or `pcallReentry`; terminal diagnostics report
`hard_limit`, with `soft: null` because local guards have no Soft threshold.
Observed counts report the rejected 257th host call or 17th re-entry. Allocator
request size is `null` when the public API cannot provide it and is never
guessed.

An earlier independent failure, Hard failure, or deadline is preserved;
cleanup cancellation may be replaced only by a later proven local Hard cause.
Codec, module-shape validation, and context construction record a public-API
`MemoryError` before conversion. An unknown panic from a direct allocation API
is not guessed to be memory exhaustion; it remains an execution exception at
the existing panic-recovery boundary. Because the adopted VM dependency can
classify an exception as `MemoryError` from its text alone, an explicit request
call to the original standard `error` function is recorded independently
before forwarding its arguments. The original function remains in the VM
registry, avoiding an ownership cycle through a Rust closure that captures the
VM handle.

Exceeding a local VM deadline or memory limit fails the whole request even
within the shared five seconds. This design adds no new busy-loop watchdog,
oversized timer, or strict operating-system real-time guarantee. SDL, type,
authorization, and reference consistency; input size; recursion depth; list
length; and regex construction remain precondition or Hard checks and are not
relaxed into Soft warnings. Existing startup Hard rejection and the local
limits and known interruption limitations of regex construction also remain.

Read-only `after` calls to `find` or `count`, and their observation copies, do
not revalidate stored patterns or start new regex matching. This preserves the
ADR-0038 rule that storage constraints do not apply to search operands. RA-07
verifies regex accounting across `after` and a later root's overflow without
adding read-only authority, new search semantics, or a test-only regex API.

Real HTTP verifies successful completion after crossing Soft. Tests do not
raise or disable limits. Any numeric change requires measurements and an ADR
amendment with rationale; the former 3:1 split cannot remain under a Soft label.

## 7. Failure responses and explicit rejection

| Internal result | State | HTTP and body |
| --- | --- | --- |
| All processing and encoding succeed | Commit once; Query makes no change. | Normal success response. |
| User explicitly rejects | Discard every candidate. | Requested status, `data: null`, and an errors array. |
| Resolver, hook, Rust processing, field, or output-validation error | Discard every candidate. | HTTP 500, `data: null`, and safe errors. |
| Hard limit or execution timeout | Discard every candidate. | HTTP 500, `data: null`, and a fixed classification code. |
| Cancellation or shutdown before commit | Discard every candidate. | Return a failure response when possible; delivery to an already closed socket is not guaranteed. |
| HTTP, document, variable, or authentication error before execution | No change. | Preserve the applicable 4xx transport contract; do not convert it to 500. |

Internal resource exhaustion and execution timeout use HTTP 500. The HTTP status alone does not distinguish causes; `extensions.code` carries a fixed classification such as `MOCK_EXECUTION_FAILED`, `MOCK_EXECUTION_LIMIT_EXCEEDED`, `MOCK_EXECUTION_TIMEOUT`, or `MOCK_REJECTED`, together with a `requestId` from the common error type.
The response excludes partial data, uncommitted Mutation results, stacks, source text, and provider information.
Existing controlled-transport behavior such as a bodyless response belongs to
the entry-point contract; it does not permit partial success during execution.

`ctx.reject({code=200,message=...})` remains a sticky rejection; status 200 does not make the request successful.
Earlier roots in the request roll back, and catching the rejection with `pcall`
and returning a later value cannot commit them.
Only an explicit user rejection may select status 200 for a failure response.
The GraphQL body is `{"data": null, "errors": [...]}` and contains neither a
custom singular `error` field nor partial data on rejection.

Permitted rejection statuses are 200 and ADR-0036's fixed set of 4xx and 5xx values. Other 2xx or 3xx values, requests for a bodyless response, unbounded values, non-integer values, strings, 600, and malformed payloads become safe HTTP 500 setup errors.
The explicit rejection message is user-published response content and is never replaced with an internal exception message.

Internal failures take precedence over rejection. A parallel field cannot hide an internal failure merely because a status-200 rejection arrived first.
Multiple valid explicit rejections use the existing maximum-status rule, independent of arrival order; this selection never affects the commit decision.
Before a worker returns, the request retains the highest valid status from every
rejection that actually ran. Dropping the receiving future does not lose that
record.
Cancellation of jobs that never ran is not counted as a new rejection, and an
internal failure takes precedence over the retained maximum reject status.
Internal cleanup cancellations issued by Rust after failure do not overwrite the original cause.
When communication remains possible, every unfinished job is brought to a
state that cannot commit before the failure response is returned.

## 8. stderr diagnostics and panics

Record exactly one terminal result per request, including HTTP status and
`committed`, `rolled_back`, or `not_started`.
Classify Soft-limit events as warnings, execution failures as errors, and user
rejections as denials. A status of 200 must not hide the fact that execution did
not succeed. Diagnostics use the existing bounded asynchronous stderr path and
retain phase, coordinate/path, request ID, fixed cause, terminal result, limit
name, Soft/Hard classification, consumption, and whether later processing was
unreached as safe structured values.
Soft-warning phase, coordinate, and path are copied from the immutable work
handle for the reached operation into the ledger. The shared ledger contains no
mutable "current field", so sibling fields and delivery-time operations cannot
overwrite the location. Resolvers use their schema coordinate and response path
including aliases; hooks use one of the six fixed slot names and the applicable
field location. Admission-wait time is reported at request scope, with no
coordinate and an empty path, rather than inventing a field location.
The synchronized observation copy is exposed as `request.view`,
`operation.view`, or `field.view`, matching the corresponding `after` slot.
Switch the copied location handle only while holding the budget lock and restore
it afterward. Node, byte, and failure-ledger charges are not refunded. No await
or VM execution occurs while that lock is held.
The same sticky failure is not repeated for every field, and independent causes
are collected only within existing finite limits. If an entered operation or
field processor never reaches the call boundary for an enabled `after` hook,
the slot is recorded in terminal `unreached_after_hooks`. Early return, future
drop, and observation-conversion failure are included. Repeated instances of
the same `field.after` are aggregated into three fixed slots; disabled hooks and
unselected fields are not listed. Reaching this call boundary does not imply VM
startup, completion, or success.

Attach Luau's safe relative module name, line number, and any stack frames that
can be obtained as separate diagnostic fields.
Never expose arguments, headers or tokens, seeds, locals, raw scripts, absolute
paths, or provider source in stack output.
Stacks are truncated explicitly within the existing output limit; when no stack
can be obtained, location and classification remain. Failure to capture a stack
never permits a commit. Collection occurs only at
boundaries where the safe public VM API exposes a host callback, interrupt, or
re-entry-limit failure. At most 16 frames with positive line numbers from the
same module are retained, and greater depth sets `truncated`. Collection is
attempted at most 16 times per VM. A stack caught by `pcall` is cleared, but the
attempt count is not refunded. Pure Luau exceptions that release their failure
frames before returning to Rust report `stack: null`; a previous successful
stack is never reused as the failure location. String allocation inside the VM
API counts toward the finite scan budget.

Ordinary Rust errors propagate through `Result`. A panic recoverable at an
existing safe catch boundary discards the candidate. The design neither causes
a panic merely to capture a backtrace nor adds indiscriminate `catch_unwind` or
a broader global panic hook. A worker whose integrity cannot be restored is not
reused or detached indefinitely. Verify that the same server recovers after a
failure that can be collected safely.

Bound disconnection and output failure on stderr itself. Do not add persistent
log storage, guaranteed delivery, or unbounded extension while waiting for
output.
Log delivery failure never relabels committed state as uncommitted.

## 9. Responsibilities for form, compatibility and implementation

Because this is not yet a production release, do not retain the old behavior
mode, reader, or aliases, and do not silently accept different atomic semantics
under the same configuration version. Backward compatibility for configuration
layout, keys, and format is not required; the parser accepts only the current
format. There is no fallback to old formats or automatic conversion. Behavioral
compatibility is limited to existing valid behavior expressed in the current
format. Apply this ADR's new rules to the atomic semantics, failure responses,
and limit behavior it changes explicitly. Update writers, readers, references,
proposals, management metadata, and fixtures together according to this initial
version table.

| Contract | ADR-0038 candidate | New version |
| --- | --- | --- |
| project | pythia-mock-project/v3 | pythia-mock-project/v4 |
| portable bundle | pythia-mock/v5 | pythia-mock/v6 |
| host API | pythia-mock-api/v4 | pythia-mock-api/v5 |
| reference | pythia-mock-reference/v4 | pythia-mock-reference/v5 |
| proposal | pythia-mock-proposal/v4 | pythia-mock-proposal/v5 |
| update-state | pythia-mock-update-state/v3 | pythia-mock-update-state/v4 |
| schema-snapshot | pythia-mock-schema-snapshot/v3 | pythia-mock-schema-snapshot/v4 |

`directives/v2`, formatter style v1, inference envelope v1, regular generated
ABI 6, `config-v2`, and `SchemaDigest` remain unchanged.
Every reader receives the new contract identity, and tests ensure old proposals
or checksums cannot enter the new behavior.
Preserve the old user project, SDK, U, `.dist`, backup, and proposal; migration
occurs through a separate empty directory and explicit initialization.
Editing protection, rename exchanges, and restoration guarantees within the
current format are not weakened.

Implementation stays within the Mock coordinator, store, worker, and HTTP
adapter; it does not create a second GraphQL executor or VM or a general-purpose
transaction framework. Any common `Endpoint` change is the smallest opt-in
extension and includes tests preserving regular consumers' partial-success and
status behavior and ABI 6.
Development preserves the 400-line handwritten limit, English rustdoc, product
ownership checks, cache reuse, and final full validation.

## 10. Acceptance

RA-01 through RA-18 Passed locally on 2026-09-14. Individual assertions, the
full command suite, measurements, and review evidence are mapped in the
verification record.
The historical baseline began with 101 successful commands. The designer's final implementation review passed on 2026-09-14 with no findings at any severity.
Remote CI, human UI/IME verification, and real AI connectivity remain
unconfirmed.

| ID | Required evidence |
| --- | --- |
| RA-01 | Commit changes from multiple roots and models in one request exactly once; later roots read earlier candidate changes. |
| RA-02 | A second-root failure discards earlier records, edges, ordinals, identities, and capacity accounting. |
| RA-03 | Nullable-child errors and output type/non-null violations produce `data: null`, status 500, and complete discard; valid null/empty values succeed. |
| RA-04 | Verify all six hooks' authority and candidate snapshots; every `after` runs before commit, and `after` exceptions or invalid returns discard everything. |
| RA-05 | Crossing the Soft boundary still runs all hooks and commits, warning once, with the same normal capacity when `after` is disabled. |
| RA-06 | Verify each common Hard limit exactly and one over, aggregation across phases, sticky failure after capture, and recovery of the same server. |
| RA-07 | Real work/value exhaustion in `after` discards earlier roots; regex accounting is not restored across read-only `after`, and later overflow or earlier failure never becomes success. |
| RA-08 | Verify continuation after the time Soft threshold, local VM/request timeouts, permit waits, encoding deadlines, and complete discard. |
| RA-09 | Verify the precedence among normal 200 responses, authorization rejection, internal 5xx failures, panic/cancellation, and request-budget exhaustion without committing partial state. |
| RA-10 | Reject invalid status/payload combinations with a safe fixed 500 response; parallel completion order must not change commit or status. |
| RA-11 | Final candidate or encoding/response-cap validation failure remains uncommitted; a fixed error response remains available after budget exhaustion. |
| RA-12 | Use barriers to verify both cancellation/commit orderings; transmission interruption or a missing commit acknowledgement causes neither double commit nor false rollback. |
| RA-13 | Verify no lost updates across concurrent Mutations, cancellation while waiting, complete old-or-new Query snapshots, and no candidate exposure. |
| RA-14 | Verify Rust `Result`, Luau exceptions, recoverable panics, release of every worker and permit, and subsequent request recovery. |
| RA-15 | Verify stderr phase, cause, transaction, request ID, and limit values; secrecy; bounded stacks; duplicate suppression; and output disconnection. |
| RA-16 | Synchronize all seven formats, reject old formats, migrate through a new directory without damaging U, `.dist`, or recovery material, and keep reference output consistent. |
| RA-17 | Exercise two domains, rename, offline operation, real CLI/HTTP, GraphiQL, fake infer/apply, TUI, and existing regressions. |
| RA-18 | Verify regular Rust 1.85, Mock Rust 1.95, stable Rust, ABI 6, every SDK exact/quality check, common full validation, release measurements, and independent reviews. |

Alias, fragment, variable, directive, and valid nullable GraphQL semantics must
not be confused with Mock's atomic-failure policy.
Tests must exercise GraphQL parallel fields under the atomic policy rather than
serializing, skipping, renaming, or deleting existing assertions.

## 11. History and non-goals

The ADR-0038 final candidate's 101-command success and release measurements are
historical records from the
previous completion ticket, retained with the private execution archive.
Its source hash is not reused to claim tree identity after documentation changes
or to pass this ADR, and evidence is not overwritten for appearance. The 3:1
proposal was rejected and does not require another approval cycle. Automatic
remote setup, SSH/package/publication/tag gates, real AI communication,
persistence, client-coordinated commit, and production sandbox guarantees are
out of scope.
