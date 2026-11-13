# ADR-0046: Standard os time and explicit timeleap control

Status: Accepted

Date: 2026-09-21

Implementation: Implemented; TL-01 through TL-17 are accepted on final source

## 1. Context and delivery boundary

[ADR-0045](0045-stateful-mock-authoring-and-runtime-contract-parity.md)
introduced `ctx.clock` with captured request time and custom millisecond-based
parsing, formatting, and arithmetic. Subsequent design discussion selected a
smaller boundary: use Luau's standard `os` functions for ordinary time handling
and a separate `timeleap` global solely to control the apparent current time.

Request-local time means one complete GraphQL request, including its hooks,
root fields, and nested resolvers. It does not mean one Luau VM, resolver,
authenticated user, or client session. Some tests also need a shared clock
that survives multiple HTTP requests to the same mock server.

This is a separate follow-up decision, not a correction to the acceptance
history of ADR-0045. Its implementation has its own TL acceptance evidence and
does not reopen or rewrite earlier review gates. Keep the existing ADR-0045
implementation evidence as historical baseline evidence.

The implementation goal prompt
is the self-contained authorized handoff. Implementation progress alone does
not mark TL acceptance passed; final evidence belongs in the
timeleap validation record.

At that cutover, this ADR supersedes ADR-0045 Section 7, the blanket `os`
exclusion in Section 3, and the affected clock discovery, format, and SA-08
assertions. Its other storage, provenance, diagnostics, ownership, and atomic
execution contracts remain in force. Framework APIs, generated Rust ABI 6,
and the existing normal-product/Mock MSRV boundary are unchanged.

## 2. Decision

- Expose Luau's four standard `os` time functions, subject to the existing
  sandbox resource and execution-phase boundaries.
- Expose `timeleap` directly as a host-provided global API table, not under
  `ctx`, `clock`, or `time`.
- Provide only `timeleap.travel_to` and `timeleap.reset` initially.
- Both operations accept an optional closed `{ world = boolean }` table.
  Omitted options, an empty table, or an omitted `world` mean `false`.
- `world = false` selects the current request; `true` selects the containing
  mock server. One mock-server instance is one world. This never selects the
  host OS, every server in the process, or a distributed set of servers.
- Reset restores the selected scope's baseline: the captured server policy
  for a request, or the immutable startup policy for a world. A fixed baseline
  remains fixed; reset is not an unconditional switch to real time.
- Remove `ctx.clock` without aliases or compatibility adapters. Do not add
  Moment-like helpers or another application-time reading API.

Global name visibility and mutation authority are independent. A host closure
can resolve its request/controller from execution ownership without requiring
an explicit `ctx` argument. The API table and its host-owned bindings cannot
be modified to escape the capability boundary.

`world` names the extent of the time change, not a capability grant or a
selectable world identifier. `world = true` still requires dedicated
server-control authority and only affects subsequently created request
contexts. The earlier proposed `global` option is not an alias; reject it even
when false or supplied together with `world`.

## 3. Standard time behavior

The public starting point is the
[Luau os library](https://luau.org/library/#os-library), verified against the
pinned Luaur/Luau implementation before publication:

| Function | Role | Effect of timeleap |
| --- | --- | --- |
| `os.time()` | Current Unix timestamp in seconds. | Read the effective apparent current time. |
| `os.time(date_table)` | Construct a timestamp from UTC date fields. | None. |
| `os.date(format?, timestamp?)` | Format or decompose a timestamp; `!` selects UTC. | Substitute current time only when the timestamp is omitted or nil. |
| `os.difftime(a, b)` | Difference in seconds. | None. |
| `os.clock()` | High-precision duration measurement with an unspecified baseline. | None. |

Preserve standard defaults, explicit/nil argument handling, date fields,
format directives, timezone behavior, and native error/nil cases. Generated
types must describe actual return possibilities, not hide a native nil case.
Do not force UTC for local formats or introduce implicit date-string parsing.
There is no additional month arithmetic, timezone database, or formatting DSL.

Implementation may intercept the current-time branch of `os.time` and supply
an explicit timestamp to the native `os.date`. This is clock substitution,
not a second date implementation. Explicit date conversion and formatting
continue to use the standard implementation. Function aliases must resolve
the apparent time when called, not retain an earlier real-time function that
bypasses substitution.

System mode reads real time on each call. Capturing a request's clock policy
does not freeze its system time. Fixed mode returns the selected instant until
the applicable policy is changed or discarded. There is no shifted-but-running
mode, accelerated clock, or sleep facility in this decision.

Only Luau-visible wall time is substituted. Host clocks, `os.clock()`, VM
deadlines, cancellation, queue waits, access-log timestamps, shutdown timing,
and automatic ID generation retain their existing independent clocks. Moving
time does not execute scheduled work, settle rewards, or modify stored data;
business code must observe time and perform those actions explicitly.

## 4. Public control API

The logical signatures are:

```luau
type TimeleapOptions = { world: boolean? }

type TimeleapLibrary = {
    travel_to: (timestamp: number, options: TimeleapOptions?) -> (),
    reset: (options: TimeleapOptions?) -> (),
}
```

These are dot calls, not receiver-method calls. The runtime validates the
closed options table as well as the generated types. Reject unknown fields,
`scope`, `global`, non-boolean `world`, wrong arity, and unavailable execution
phases. Do not apply truthiness conversion. Nil options have the same meaning as
omitted options. The absence of a request never changes the default to
world-wide control.

`travel_to` accepts a finite, exactly representable integer Unix timestamp in
seconds. It rejects strings, date tables, and fractions. Every numeric input
means seconds; there is no millisecond auto-detection or unit conversion.
Validate native timestamp conversion bounds before changing any state. An
admitted value must have a valid native UTC decomposition that round-trips
through native `os.time` to that value. Apply safe numeric checks before native
conversion; do not rely on overflowing casts or a panic as validation.
Publish and test the pinned implementation's supported boundaries. Do not
carry ADR-0045's custom RFC3339 parser or its calendar-year restrictions into
the public `os` functions.

Repeated `travel_to` replaces the selected fixed value; it does not push a
restoration stack. Repeating `reset` is harmless in an authorized phase.
Neither operation returns a clock object or a timestamp. Relative movement
can use ordinary arithmetic in an authorized control context; a separate
`travel_by`, `advance`, or calendar helper is not part of the initial API.

```luau
-- Request preparation: both calls select the current request.
local target = os.time({
    year = 2026, month = 9, day = 20,
    hour = 9, min = 0, sec = 0,
})
assert(target ~= nil)
timeleap.travel_to(target)
timeleap.reset()
```

```luau
-- Dedicated server-control execution: world must be explicit.
timeleap.travel_to(1789894800, { world = true })
timeleap.reset({ world = true })
```

## 5. Ownership, inheritance, and reset

Keep two small host-owned policy layers:

1. A server clock: an immutable startup policy and a current policy, each
   `System` or `Fixed(timestamp)`. Initialize both from the configuration
   loaded at server startup; travel/reset changes only the current policy.
2. A request view: the server policy captured at request-context creation,
   plus an optional request-local fixed override.

Create the request view before `request.before`. This is the clock's
linearization boundary, not socket acceptance or the creation of each VM.
A request waiting to obtain an execution context observes the policy current
when that context is created. All VMs participating in it share the same view.

| Operation | State transition | Lifetime |
| --- | --- | --- |
| `travel_to(t)` or `travel_to(t, { world = false })` | Set the request override to `Fixed(t)`. | This request only. |
| `reset()` or `reset({ world = false })` | Clear the request override; resume its captured server policy. | This request only. |
| `travel_to(t, { world = true })` | Set the server policy to `Fixed(t)`. | Later request contexts until changed or server shutdown. |
| `reset({ world = true })` | Restore the server's current policy to its immutable startup policy. | Later request contexts until changed or server shutdown. |

The request override takes precedence over its captured policy. A server
change never edits an existing request's captured policy. Consequently:

- A request that inherited server time A, then selected local time B, returns
  to A after local reset, even if the server has meanwhile moved to C.
- A request that inherited `System` resumes the then-current real time after
  local reset, not the real timestamp from request creation.
- A world started at `Fixed(A)`, then moved to B and C, returns to `Fixed(A)`
  after world reset, not B or System. Further resets keep A.
- A world started in `System` resumes progressing real time after world reset,
  not the timestamp at server startup. Travel/reset never changes its baseline
  or reloads configuration files.
- World reset does not cancel another request's local override or retroactively
  unfreeze an existing request that captured a fixed server policy.

Discard all request-local state on success, rejection, failure, cancellation,
or timeout. No authentication/session map, VM-local clock, cross-server static,
or user-selectable inheritance chain is introduced. Independent concurrent
test scenarios use separate server instances.

## 6. Availability and mutation authority

Clock reads and clock controls have different permissions:

| Execution phase | Time reads | Request control | Server control |
| --- | --- | --- | --- |
| Configuration and module initialization | Explicit-input conversions only. | No. | No. |
| `request.before` and `operation.before` | Request view. | Yes, before field execution. | No. |
| Root/nested resolvers, `field.before`, and all after hooks | Request view. | No. | No. |
| Host-owned request preparation for non-HTTP tests | Request view. | Yes. | No. |
| Dedicated server-control execution | Its candidate server policy. | No request; reject. | Yes, with `world = true`. |

Freeze request policy mutation when preparation completes, before any root or
nested resolver is scheduled. Sequential preparation hooks see successful
earlier local changes. After hooks observe the final request view but cannot
reset it. Host cleanup, rather than a user-written after hook, owns disposal.

Configuration and module initialization must not capture current wall time or
elapsed host time. Pure explicit-input `os` conversions remain usable there;
current-time reads, `os.clock`, and timeleap effects require an appropriate
live execution context. Merely defining a callback that uses runtime APIs is
not an initialization effect. A top-level call must fail during the same
initialization evaluation used by check and serve.

Generate phase-aware declarations and reference availability from the shared
capability registry. Enforce authority again in Rust, including calls through
aliases, weakened types, `_G`, and `pcall`. An invalid control call is a sticky
internal failure and cannot become a successful request by catching it.
Neither an HTTP header nor a GraphQL argument grants server-control authority.

## 7. Dedicated control boundary and concurrency

Provide a server-owned control handle and a bounded Luau control runner for
embedding and in-process test orchestration. A control execution has `os`,
`timeleap`, and the permitted bounded standard primitives, but no request,
model-write API, or additional filesystem/network/process authority. This
makes the same `timeleap` contract usable between actual HTTP requests without
adding a test-only GraphQL field to the public schema.

Serialize control executions per server. Evaluate against a private candidate
policy and publish it once, only after successful completion and the final
failure/cancellation fence. Reads inside the control execution see its own
candidate. Failure after a successful `travel_to` must discard the entire
candidate, including failures caught by `pcall`. Use the existing bounded VM
admission, execution, and recovery machinery rather than an unbounded REPL.

Publishing a policy and capturing a request policy must be atomic with respect
to each other. Do not hold a policy-publication lock across a VM execution or
an HTTP request. Requests see either the prior or published policy, never a
partially updated state. Return control success only after publication; a test
can await that result before issuing its next HTTP request.

Server control is separate from the GraphQL data transaction. A later failed
business request does not undo a previously completed control operation.
Conversely, failed request-local preparation cannot change the server clock
or commit data. World reset changes time only, not seeds, data, or sessions.

The initial transport boundary is deliberately limited to host-side control.
This ADR does not authorize a public HTTP admin route, browser header override,
CLI-to-running-server IPC protocol, or arbitrary remotely supplied scripts.
Those require a separate transport/security decision. Do not advertise remote
control of a running CLI instance until such an interface exists. Request-local
hooks and declarative startup settings remain usable by the standalone CLI.

## 8. Startup configuration, export, and clean break

Keep startup clock policy as declarative configuration, not a callable
`ctx.clock` compatibility surface. The new configuration uses Unix seconds:

```luau
clock = { mode = "system" },
```

```luau
clock = { mode = "fixed", now = 1789894800 },
```

`init` selects system mode. A fixed `now` follows the same admission rule as
`travel_to`. Each server retains its configured startup policy as an immutable
baseline. World reset restores that policy: a fixed startup returns to its
original fixed instant, and a system startup returns to progressing real time.
Reset does not reread configuration, restore a previous travel target, or
sample a timestamp to represent System. A new server start loads its startup
configuration anew; changes on disk affect that new instance, not the running
instance's reset baseline. Request reset only resumes its captured server
policy, which may differ from the server's startup policy.

There is no operation or option in this initial API to force a fixed-startup
world into System mode. A dedicated switch to real time is deferred until a
concrete use case warrants a separate design decision; do not implement one
as part of this ADR or give `reset` that second meaning.

Export serializes the declared startup policy and user-owned sources, not a
running server's changed policy or request overrides. Configuration/generation
identity binds that declaration and the capability version, not a sampled
current timestamp. Pure constant `os` conversions in configuration must not
turn init/update/check/export into time-dependent generation. Preserve the
existing distinction between target checks and whole-project readiness;
neither executes arbitrary business or live-control scenarios.

Follow the already approved complete compatibility break. Remove old clock
helpers, string-valued fixed timestamps, aliases, and obsolete declarations
instead of maintaining two clocks or silently converting units. Reject old
or mixed generated artifacts before changing files. Transition through a
fresh directory with explicit adaptation of user-owned sources. Preserve
current-format `.dist`, transaction, recovery, and unmanaged-file protections.

Relative to the ADR-0045 baseline, the planned contract versions are:

| Surface | ADR-0045 | This follow-up |
| --- | --- | --- |
| Project | v5 | v6 |
| Portable bundle | v7 | v8 |
| Host API | v6 | v7 |
| Capabilities | v1 | v2 |
| Full/topic reference | v7 / v3 | v8 / v4 |
| Update state/schema snapshot | v6 / v6 | v7 / v7 |

Check diagnostics v2 and runtime logs v2 retain their existing shapes. If
another accepted change consumes a planned version before implementation,
rebase the version inventory first; never reuse a version for a different
contract or add a compatibility reader to avoid that rebase.

## 9. Implementation and discovery plan

Keep the controller independent of calendar conversion and the data store.
The current integration points are `api/runtime.rs`, `sandbox.rs`,
`runtime/execution.rs`, `runtime/context.rs`, `runtime/clock.rs`, `clock.rs`,
and `service/hooks/session.rs`. These identify responsibilities, not a mandate
to retain the old file layout or old millisecond implementation.

One registry must supply runtime installation, phase-aware checker types,
generated declarations, reference text/JSON, and executable examples. Extend
`reference runtime stdlib` with `os`, replace `reference runtime clock` with
`reference runtime timeleap`, and document defaults, permissions, seconds,
reset baselines, and request-local versus world-wide lifetime. Explain that
request reset resumes its captured policy while world reset restores startup
configuration without rereading it. Do not retain the old topic as an alias.
Update generated agent guidance and resolver/hook discovery.

Bound native date-format input/output, table conversion, allocation, and host
calls with the existing limits. Validate dangerous numeric conversions before
native entry; audit native calls rather than assuming bytecode interrupts
bound them. Preserve sanitized diagnostics with logical source positions,
safe expected/actual structure, and fixed hints. Do not publish raw native
exceptions, request values, physical paths, or clock-control script contents.

The authorized implementation follows this order:

1. Reconcile the then-current baseline, pin native `os` behavior and numeric
   boundaries, and finalize the new version inventory and acceptance fixtures.
2. Add bounded standard `os` installation and shared declarations, including
   explicit-input conversion and phase/nil/error parity.
3. Replace captured-instant storage with server policies and request views;
   implement request preparation, travel/reset, sealing, and automatic cleanup.
4. Add the server-owned control runner, immutable startup baseline, reset
   restoration, candidate publication, authorization, failure fencing,
   concurrency, and same-process multi-server isolation.
5. Cut over configuration, generated artifacts, export, references, guidance,
   and architecture/handoff/conformance documents; remove obsolete APIs.
6. Run the full acceptance and repository gates and record new evidence.

## 10. Acceptance criteria

All entries start as Required / Not run. Previous passing test counts are not
acceptance evidence for the changed clock contract.

| ID | Required evidence |
| --- | --- |
| TL-01 | Published `os` members type-check and execute, including overloads, nil/default arguments, native failure results, and UTC/local formats; unrelated OS capabilities remain unavailable. |
| TL-02 | System-mode time can progress within one request; fixed time remains constant. Explicit-input conversion, `os.clock`, and host deadlines are unaffected. |
| TL-03 | Direct `timeleap` calls work in authorized phases. Omitted/nil/empty options and explicit `world = false` are request-local; `world = true` selects exactly one mock-server instance. No implicit promotion occurs without a request. |
| TL-04 | Invalid timestamps, fractions, strings, nonfinite/unsafe numbers, native boundaries, unknown options, non-boolean world, and wrong arity fail without changing effective policy. |
| TL-05 | Root and nested resolvers and hooks share the request view across fresh VMs. Overrides cannot leak to another request or survive completion. |
| TL-06 | Local reset restores the captured server policy, not the latest or startup policy; system inheritance resumes progressing real time. World reset restores the immutable startup policy, not the preceding travel target. Repeated resets are safe. |
| TL-07 | Server travel/reset changes later contexts only. In-flight and queued-before-context cases assert the documented linearization boundary. |
| TL-08 | Request and server authority cannot be obtained through `any`, aliases, dynamic global lookup, `pcall`, forged options, headers, or business inputs; field/after mutation is rejected. |
| TL-09 | Initialization permits pure conversions but rejects live clock reads/control. Defining runtime callbacks is valid; capability discovery accurately describes phase restrictions. |
| TL-10 | Rejection, invalid hook output, later-root/after failure, timeout, cancellation, and VM recovery discard local state and preserve existing data rollback. |
| TL-11 | A failed or cancelled control execution, including one that called reset, publishes nothing; a successful execution publishes once. The startup baseline remains unchanged. Concurrent controls are ordered and other server instances are unaffected. |
| TL-12 | An in-process controller drives real HTTP create, world leap, and expiry-check requests. Local override/reset and world reset are asserted by values, not status alone. |
| TL-13 | Fixed startup A, travel to B and C, then world reset returns to A; system startup resumes progressing real time. Reset never reloads configuration, while a new server start uses its newly loaded configuration. Live changes never alter export or configuration identity. No force-System control API is exposed. |
| TL-14 | Old ctx.clock APIs, scope/global options (including global=false and mixed global/world tables), old formats, and string-valued clock configuration are rejected without mutation; fresh-directory adaptation and current-format ownership protections pass. |
| TL-15 | Time reads through aliases cannot bypass substitution; large formats/native conversions obey allocation/work limits under watchdogs and permit subsequent VM recovery. |
| TL-16 | Reference and generated guidance examples pass checks and execution in their stated phases; full/target readiness retains its existing meaning. |
| TL-17 | Implementation rules, formatting, Clippy, rustdoc, focused/full tests, exact generated contracts, dependency checks, and the existing validation gates pass on final source. |

Include interleaving tests with an injected real-time source so concurrency
assertions do not depend on sleeps or wall-clock luck. Exercise debug/release
VM recovery and a renamed or second schema. Record unsupported/unexecuted
platforms and any pre-existing validation failures separately. Change the
implementation status only after the new acceptance evidence is complete.

## 11. Consequences and rejected alternatives

Authors learn one standard date API and two control operations. Request-local
behavior is the safe default; world-wide effects remain explicit and authorized.
The host owns two small policy layers and retains one immutable startup
baseline rather than a general date library, session clock registry, or
process-wide time patch. Reset consistently restores the selected scope's
baseline, preserving fixed test configuration rather than silently enabling
real time. An explicit force-System control is deferred, not part of this API.

The cost is explicit lifecycle and publication logic, phase-aware capabilities,
and a clean artifact transition. A frozen time source alone does not make a
scenario deterministic: local timezone behavior, generated IDs, and business
logic remain separate concerns. Shared world state can still interfere with
tests sharing one server, so independent scenarios must use separate instances.

Rejected alternatives are retaining `ctx.clock` as sugar, making timeleap a
clock-reading object, scope strings or a `global` option instead of the agreed
`world` boolean, world-wide effects by default, resolver-triggered shared
changes, resetting unrelated requests, unconditional System reset, exposing
the complete Lua OS library, changing host time, implicit restoration stacks,
and compatibility shims for the unreleased clock API.
