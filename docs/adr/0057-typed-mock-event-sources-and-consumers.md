# ADR-0057: Typed Mock event sources and transactional consumers

Status: Accepted / Implemented with focused local macOS acceptance

Date: 2026-10-25

Related: ADR-0031, ADR-0044 through ADR-0047, and ADR-0051 through ADR-0056.

Tracking: Beads `lab01-u3c` covers the historical design publication only.
Implementation is tracked by `lab01-c15`; independent review decisions and
correction verification are recorded in `lab01-c15.4.1` and `lab01-c15.4.2`
under the [review workflow](../review-workflow.md). The
validation record identifies executed
EV-01 through EV-12 evidence and unexecuted qualification scope.

[Architecture Section 52](../architecture.md#52-typed-mock-event-sources-and-transactional-consumers),
[Handoff Section 44](../implementation-handoff.md#44-typed-mock-event-source-and-consumer-work-packages)
and [Conformance Section 44](../spec-conformance.md#44-typed-mock-event-source-and-consumer-conformance)
carry the corresponding implemented boundaries and work/acceptance obligations.

The implementation goal prompt
provides a standalone assignment with focused local validation and independent
reviews. Preparing that prompt does not launch implementation.

## 1. Context and decision

Before this ADR, Mock offered explicit post-commit publication from Mutation
resolvers and a topic binding returned by Subscription setup, named
`ctx.publish.Subscription.<field>` and `ctx.subscription:bind`. This ADR
replaces those APIs. Model changes still do not automatically publish events.

Mocks also need events without a Mutation: a finite dataset should advance on
a timer and optionally repeat. A consumer asking for the next value cannot be
the clock for this requirement. A slow consumer must not pause generation or
cause unlimited retained work.

Separate typed event production from its consumers. Provide pub/sub sources
and scheduled sequence sources behind shared host-owned delivery machinery.
GraphQL subscriptions consume these sources. Named server-lifetime Luau
consumers can use the same sources to log, update models and publish further
typed events. The adopted SDL payload type names identify public event APIs;
Subscription fields remain typed delivery destinations. A public `any` payload
or untyped event bus is not introduced.

Keep source waiting, scheduling, queues and cancellation in the host. Luau
returns an opaque typed source and executes a short callback for each event.
It does not return an arbitrary retained closure, suspend a VM or keep its
captured request context alive between events.

## 2. Amended and preserved boundaries

This ADR changes ADR-0054 Section 7's Mock names, one-exact-topic source
restriction and Mutation-only publication authority. It adds scheduled sources,
registered non-GraphQL consumers and their transactions. Consumer execution is
an explicit new privilege; the six existing request/operation/field hooks,
Query resolvers and GraphQL event resolvers remain unable to write or publish.

Preserve request-atomic Mutation behavior, bounded post-commit dispatch,
immutable captured values, private Record/DTO boundaries, fresh GraphQL event
context and the three transport profiles. The application-facing Rust resolver
API, generated Rust ABI 8, SDK lifecycle ownership, tracing registration and
default dependency boundaries are unchanged. Internal delivery policy and
resource ownership should be shared without creating a general public Rust
broker API or duplicating policy in HTTP adapters.

This does not add network/filesystem access to Luau, `require`, durable events,
replay, acknowledgements, retries, remote administration, runtime consumer
registration, a new process supervisor, or an external broker. Reuse the
existing released Luau dependency; no upstream patch or copied checker is
authorized. Each source/consumer module is checked independently.

## 3. SDL-type source and publication APIs

Generate ordinary dot-call functions using the exact SDL named output type of
adopted Subscription fields, including custom-named roots. Deduplicate repeated
types. Query references neither prevent generation nor grant publication
privileges. For `type Item`, `Query.a: Item` and `Subscription.b: Item`, expose
`ctx.pubsub.publish.Item`, `ctx.pubsub.subscribe.Item` and
`ctx.generator.sequence.Item`; there are no `b` aliases. Types used only by
Query or Mutation do not acquire event APIs.

Examples below assume `messageAdded: Message!`:

```luau
return ctx.pubsub.subscribe.Message({
    topic = "room:" .. args.roomId,
})
```

```luau
ctx.pubsub.publish.Message({
    topic = "room:" .. args.input.roomId,
    payload = message,
})
```

```luau
return ctx.generator.sequence.Message({
    values = {
        {id = "sample-1", text = "Hello"},
        {id = "sample-2", text = "World"},
    },
    interval_ms = 1000,
    loop = true,
})
```

The last example assumes `messageAdded` returns a `Message!` with the shown
fields. Exact generated types and existing stored-value rules remain the
authority. Each `values` element is one complete field output: a list-valued
field requires a list per element, and a nullable field can use `ctx.null`.
Neither Luau `nil` holes nor weakening a type assertion bypass host validation.

The publisher for a named type accepts the union of the complete output shapes
of its Subscription destinations. Validate list/nullability, field constraints
and provenance against each destination before capture. At least one must
match. Capture the payload once and count the call as one publication, carrying
a bounded immutable set of matching destinations. Deliver once to every matching
destination; an incompatible list shape or non-null destination is ineligible,
not a failure when another destination matches. Resource exhaustion remains a
sticky failure across all alternatives. Different SDL named types remain
isolated even when structurally identical.

Each setup context exposes only its destination's named-type selector, with
that destination's complete sequence element shape. Generate a distinct source
type per destination field. A source carries its destination identity, not just
an interchangeable payload shape. GraphQL setup must return a source
for its own field. A named consumer must return a source for its registered
field. Wrong-field, forged, foreign-owner and reused source handles are rejected
without activating a source. Only the source actually returned by that setup
is activated; unused constructed handles leave no timer or registration behind.

| Invocation context | Source selection | Publication and effects |
| --- | --- | --- |
| GraphQL Subscription `subscribe` | Its field's pub/sub or sequence source | Read-only models; no publication |
| Named consumer `subscribe` | Its registered field's pub/sub or sequence source | Read-only models and explicit logging; no publication |
| Mutation `resolve` | None | Existing model writes and typed publication in the complete request candidate |
| Named consumer `consume` | None | Model writes, explicit logging and typed publication in its own event candidate |
| Other resolvers and existing hooks | None | Existing privileges only |

Check permissions in generated declarations and host calls. Catching an invalid
call with `pcall` does not clear its sticky candidate failure. Source handles
and metadata are read-only capabilities, never a way to transfer a writable
context to another role.

### 3.1 Topic selection

A public pub/sub channel is the SDL named type plus a topic, scoped to one
Mock server. Delivery additionally requires the destination compatibility set
validated at publication. Exact topics retain the existing 1 through 256 UTF-8
byte bound and ASCII-control exclusion. Matching is case-sensitive and exact;
`room:*` is an ordinary literal topic.

GraphQL setup requires `{topic = ...}`. Named consumer setup can select either
`{topic = ...}` or `{all_topics = true}`, exclusively. The latter matches all
topics for that named type, including topics first used after registration,
with one delivery per compatible destination per publication. It does not match
a different named type or create wildcard patterns. Both forms optionally
accept `overflow` as specified in Section 6. Unknown keys and ambiguous selections are invalid. All-topic matching
must not retain an unbounded history or index of previously published topic
names; topic/envelope ownership remains covered by the shared retention limits.

Registration has a host linearization point. Only later committed publications
are eligible; there is no initial replay or automatic Query/subscription
continuity. A published root is captured once using the existing provenance
rules and cannot change with the caller's later table or model mutations.

## 4. Scheduled sequence semantics

`sequence.<Type>` accepts the closed input `{values, interval_ms, loop?,
overflow?}`. `values` is a nonempty dense array of at most 1,000 complete field
outputs, preserving the existing value-list limit. `interval_ms` is an integer
from 1 through 86,400,000 inclusive. Omitted `loop` is `false`.

Validate and immutably capture the complete definition during setup, before
activation. It may contain genuine model records obtained during that short
setup, but never a live Luau table, function, VM, request or writable context.
Its byte limit is independent of GraphQL response encoding. A source definition
whose single event cannot fit its configured source byte budget is rejected
before activation. Ordinary value, private-slot and capture limits still apply.

Every successful setup creates an independent timer and position. Identical
definitions are not implicitly merged across clients or named consumers.
Start the clock at source activation and generate the first item one interval
later. Later scheduled times are measured from that same origin, not from
consumer demand, callback completion or the previous delayed timer wakeup.

Use host monotonic elapsed time. Wall-clock corrections, fixed Mock time,
request-local time travel and world `timeleap` do not change the cadence.
Event execution still uses the current apparent-time policy under ADR-0046.
No source creates an event-execution context while merely waiting for a tick.

If the host resumes late, calculate missed ticks arithmetically; do not enqueue
or execute a catch-up loop. For an array of length N and interval I, the latest
due tick is `floor(elapsed / I)`. With looping, select that tick's array position
modulo N and generate at most one item. Advance past every skipped position.
Remember the last processed tick; a wakeup without a later due tick emits nothing.
For a finite sequence, clamp the latest due tick to N, generate its value once
if not already generated, then close the producer. Thus resuming after the
finite schedule ends produces the final scheduled value once, not a burst.
Drain already accepted items before ordinary finite-source completion.

Skipped timer ticks never entered a queue and are not delivery-drop counts.
Use checked arithmetic and the existing safe event-sequence ceiling; exhaustion
ends the affected source instead of wrapping. A timer wakeup or queue offer
does not itself invoke Luau or await a slow consumer.

Sequence output goes only to its directly connected consumer. It is not an
automatic pub/sub publication and has no topic; its event depth starts at zero.
A named consumer can explicitly bridge it into a topic using typed publish.

## 5. Named transactional Luau consumers

### 5.1 Registration and module contract

Extend the closed project configuration with optional `events`. Omitting that
group or its subfields selects default limits and an empty consumer registry.
The initializer writes an empty group, not an active background consumer.

```luau
events = {
    limits = {},
    consumers = {
        message_audit = {
            field = "messageAdded",
            enabled = true,
        },
    },
},
```

`consumers` is a map of at most 1,024 registrations, including disabled entries.
A name is 1 through 64 ASCII
bytes matching `[A-Za-z_][A-Za-z0-9_]*`; reject case-folding path collisions.
`field` is an existing adopted Subscription root field name, not a Rust name
or arbitrary type name. `enabled` is required and boolean. All registration
records are closed. Resolve the module only as `consumers/<name>.luau` inside
the config directory, with the existing no-follow and confinement rules.

Each module returns exactly two functions, `subscribe` and `consume`. For
`Subscription.messageAdded`, the generated signature is illustrated below;
normal generated naming follows the actual root and field names:

```luau
--!strict
local consumer: ChigiriSubscriptionMessageAddedConsumer = {
    subscribe = function(
        ctx: ChigiriSubscriptionMessageAddedConsumerSetupContext
    ): ChigiriSubscriptionMessageAddedSource
        return ctx.pubsub.subscribe.Message({all_topics = true})
    end,
    consume = function(
        ctx: ChigiriSubscriptionMessageAddedConsumerContext,
        event: ChigiriSubscriptionMessageAddedEvent
    ): ()
        ctx.log("info", "message received", {id = event.id})
    end,
}
return consumer
```

Consumer setup has no GraphQL argument object; required GraphQL field arguments
do not become startup arguments. Each enabled consumer selects one source once
at startup. Prepare and register all enabled consumers before accepting HTTP
requests or starting their sequence clocks. Fail startup cleanly if an enabled
module cannot establish its source or required admission cannot be reserved.
Do not leave a partially activated set. Disabled consumers never execute.

`mock codegen` generates types and absent module scaffolds for registered
consumers, preserves authored bodies and uses existing orphan/recovery rules
for removed registrations. Scaffolds use intentional `NOT_IMPLEMENTED` bodies,
without completion flags. All registered modules receive structural/type checks;
disabled modules are not silently exempt. A valid intentional error body remains
valid under ADR-0051; enabled source establishment can still fail at runtime.

### 5.2 Event execution and transactions

Keep each consumer alive without GraphQL clients. It processes its accepted
events FIFO with at most one running invocation; different consumers remain
independent. Use the existing bounded worker/scheduling and store-commit
machinery. Do not run consumer code inside a publication/store lock or create
unbounded tasks per event. On the existing single VM scheduler, fairly
interleave ready named inputs with ordinary queued jobs. Acquire write admission
without blocking that VM on a GraphQL candidate that needs it to finish. Keep
an input wholly unprocessed until write admission and invocation start; sample
source-drop metadata at that boundary.

Invoke `consume(ctx, event)` in a fresh short VM and independent write candidate.
The event is the captured typed payload, not a pre-rendered GraphQL response.
Additional reads use the model API against that candidate's current committed
base. Preserve genuine Record provenance and immutable captured roots; neither
IDs nor reconstruction can grant private or relation authority to a DTO.

The context supplies model writes, `ctx.fail`, `ctx.log`, JSON constructors,
`ctx.null` and typed publication. It has no HTTP request, authenticated client
or GraphQL selection; expose `ctx.request = ctx.null`. Do not inherit publisher
headers, state patches or local time overrides. Apparent time and execution-local
state are fresh per invocation and are not retained across events. Existing
clock-control permissions do not acquire world authority from this new role.

Expose read-only `ctx.event.topic`, `ctx.event.depth` and
`ctx.event.droppedTotal`. Topic is the exact publication topic, or `ctx.null` for
a sequence. Depth is the host-owned causal depth. `droppedTotal` is `ctx.null`
for non-dropping policy, otherwise the cumulative actual source-queue eviction
count for this consumer at invocation start, initially zero. These values describe
delivery; mutating or forging Lua values never changes the host's authority.

Commit model changes and pending publications only after the whole invocation
succeeds, including host-call and output-contract checks. Reuse existing
publication ordering, request limits, budgets and pre-commit cancellation.
Pending child publications remain invisible until commit. Never synchronously
call downstream consumers from `publish`.

Business failure, contract failure, timeout, depth violation or pre-commit
cancellation rolls back that invocation's model changes and pending events.
Record the failed result, do not retry automatically, and continue to the next
event when the worker remains usable. Preserve existing fatal worker/ownership
policies for infrastructure failure. Consumer failure does not roll back the
already committed publisher, another consumer or an earlier event.

`ctx.log` retains its existing bounded explicit-log behavior; an authored log
message is not transactional confirmation. Automatic terminal records state
committed/rolled-back/stopped status and safe identifiers/cause, without payloads,
publisher credentials or raw source. Logging pressure does not block commits
or become an acknowledgement protocol.

### 5.3 Bounded republish depth

An external Mutation publication or sequence-generated event has depth 0.
Publishing while consuming depth d creates a child at d + 1, regardless of its
field or topic. Depths 1, 2 and 3 are allowed. A consumer can process a depth-3
event and commit model changes if it does not publish again.

An attempted depth-4 publication is a sticky invocation failure, including when
caught by `pcall`. Roll back the entire consumer candidate and all its pending
publications. Record the failure and continue as above. The parent transaction
is already independent and is not undone.

Carry depth in the immutable host envelope through staging, fan-out and queues.
Do not accept a client/Luau depth override or reset it on a field/topic change.
Source construction is unavailable inside `consume`, so it cannot manufacture
a new depth-0 sequence. Fan-out preserves depth; the limit counts causal edges,
not consumer count. Branching still obeys publication, queue, worker and shared
retention limits; a depth limit alone is not a memory bound or exact cycle
detector.

## 6. Shared bounded source delivery

Reuse host delivery policies, admission/cancellation ownership and checked
accounting for both pub/sub and sequence sources and both consumer kinds.
Track each consumer's progress with an independent queue/cursor. A single
destructive queue shared by readers would distribute events rather than
broadcast them and is not this contract.

Distinguish two stages: captured typed events waiting before consumer execution,
and existing encoded GraphQL results waiting after execution. Do not apply an
encoded-byte estimate to arbitrary source values or replace existing response
limits. Internally shared immutable storage must retain provenance and charge
each physical payload allocation plus retained entry/envelope metadata before
copying or publication. Per-source queue-byte checks may conservatively charge
each retained event's full captured size even if backing storage is shared.
Global ownership charges stay alive through the last actual reference.

Use `events.limits` for source-stage limits. Every setting is a positive integer
no greater than its hard maximum; equality is valid. Omitted fields use defaults,
and unknown or inconsistent settings fail before activation. This is a bounded
design baseline, not measured production capacity.

| Configuration key | Default | Hard maximum |
| --- | ---: | ---: |
| `max_queued_events` | 32 | 1,024 |
| `max_queued_bytes` | 256 KiB | 4 MiB |
| `max_queue_age_ms` | 10,000 | 60,000 |
| `max_retained_bytes` | 16 MiB | 64 MiB |
| `max_sequence_bytes` | 1 MiB | 16 MiB |

The first three apply per active source/consumer. Count wholly waiting events,
excluding its one executing input; retain the byte charge for queued and
executing input until physical release. Sequence definitions are accounted
separately from per-source queue bytes. The global retained-byte limit covers
captured definitions, staged source payloads, retained delivery envelopes and
queued/executing source references across both consumer kinds. Preserve existing
publication-stage bounds as additional limits, not implicit enlarged allowances.

A dispatch reservation can remain physically retained by source inputs after
all staging cursors advance. Keep its pressure signal until the failed
reservation could fit again. Recover through the existing source policy of
actual publication retainers: non-dropping termination, or only their own
eligible waiting evictions followed by re-evaluation. An idle source holding
no publication input is not a recovery target. Never release a batch charge
early or enlarge the dispatch budget to hide this pressure.

Require `max_sequence_bytes <= max_retained_bytes` and preflight enough capacity
for the admitted definition and an event; this does not reserve future capacity
for an unlimited producer. Active GraphQL sources and named consumers also
share the existing configured subscription/source admission and worker bounds.
Fail startup if enabled named consumers cannot fit. The source-byte guarantee
excludes allocator overhead, VM/store working memory and existing separately
bounded encoded/wire buffers; never advertise it as a process RSS limit.

Evaluate applicable count, byte, age and shared-retention conditions with OR
semantics on admission, enqueue, dequeue and timer expiry. Source fullness never
pauses a sequence clock or blocks a Mutation commit indefinitely. Default
`overflow = "backpressure"` is non-dropping: when a source cannot retain an
event within its bounds, end that affected consumer, release its pending work
and record the cause. GraphQL receives the existing appropriate terminal error;
a named consumer stops until server restart. Other consumers continue. Existing
encoded-response backpressure still stops source demand as defined by ADR-0054.
Termination cancels current work through the existing ownership fence, with
pre-commit rollback and physical charges retained until actual release. It
never reclaims an executing value as a queue eviction or undoes a won commit.

Explicit `overflow = "drop_oldest"` may evict that consumer's oldest wholly
unprocessed source events until all recoverable conditions fit. Never evict
another consumer's entries, running Lua work, or partially sent GraphQL output.
If the new event cannot fit even after permitted eviction, or a condition is
not recoverable by eviction, terminate the affected consumer. No fixed clock,
completed transaction or unrelated subscriber is rewound to recover space.

For GraphQL, combine actual source-queue and encoded-result evictions into the
existing `extensions.chigiri.delivery.droppedTotal`. Count each event's eviction
once; never evict an already discarded event again. Source waiting does not
open event spans or execution contexts, and an unstarted discarded item creates
none. Missed generator ticks, upstream omissions, terminal/shutdown abandonment
and client/network loss are not this counter. Keep non-dropping/null semantics,
safe counter exhaustion and finalization before emission. Delivery metadata
works with logging disabled. Large source-counter advances may be coalesced into
bounded cumulative notices; coalescing never changes the exact wire/event total.
Logical terminal selection freezes both stages and stops further source
admission before physical cleanup. No new SDL field or client overflow argument
is added.

## 7. Lifecycle and project workflow

The Mock host owns active consumers, timers and worker permits through the same
bounded lifecycle root as its existing work. Endpoint/server shutdown stops new
consumer execution and generation, unregisters sources and cancels timer waits.
Immediate cancels current work under existing ownership rules. Graceful permits
only already-running invocations to finish within the existing drain deadline;
it does not start all queued callbacks or recursively drain publication chains.
Queued unstarted events are released. Preserve actual worker-join accounting
and quarantine behavior; no detached task keeps a VM, source or budget alive.

Restart restores seeds, re-establishes enabled consumers and restarts every
sequence at its first scheduled interval. There is no replay of old queues.
No-hook application lifecycle limitations under ADR-0055 remain unchanged.

Use adopted public SDL as the event contract under ADR-0052. `mock codegen`
regenerates local types/modules after consumer/configuration/storage changes;
only `mock update` adopts external public SDL. Validate a registered field before
generation; do not silently retarget a removed field or overwrite its module.

Update init guidance, generated `AGENTS.md`, scoped check discovery, runtime and
field references, enabled-consumer inventory and portable export together.
Reference shows exact type selectors and destination-specific signatures; document
`runtime pubsub`, `runtime generator` and named `consumers` reference selections.
Disabled registrations remain inspectable. Project/bundle references retain
their authority labels. Export includes the registered consumer definitions and
modules needed for the same behavior without the original schema/filesystem.

Readiness checks validate configuration, source/consumer descriptors and types;
they do not execute background callbacks, start timers, mutate models or scan
for business completeness. No dedicated test runner, checker daemon or new
unfinished-state convention is introduced.

## 8. Clean cutover and version boundaries

No backward compatibility, alias, old-format reader, automatic source rewrite
or in-place old-project update is included. Remove `ctx.subscription:bind` and
`ctx.publish.Subscription.<field>`. Retire the previous protected v8 project
update exception for the new executable. Users initialize a fresh config
directory and deliberately adapt their handwritten code, storage and seeds;
the old directory is never modified as migration machinery.

The implementation coordinates these versions across projects, bundles,
generated contracts, references, logs, fixtures and documentation:

| Surface | Previous | Current |
| --- | --- | --- |
| Mock project | v9 | v10 |
| Portable bundle | v10 | v11 |
| Host API | v9 | v10 |
| Capability registry | v3 | v4 |
| Full / topic reference | v12 / v8 | v13 / v9 |
| Runtime JSONL log | v4 | v5 |
| Update-state / schema snapshot | v10 | v11 |

Check JSON v3, schema-sync v1, SDL-normalization v1, Mock vocabulary v3 and
generated Rust ABI 8 remain unchanged. Reject old, mixed and forged inputs
before execution/listening; do not silently convert them. Examples and owned fixtures must be regenerated through normal
commands rather than hand-edited into acceptance.

## 9. Focused acceptance and implementation order

The following are normative acceptance obligations. Executed local evidence
is recorded in the validation record;
both bounded independent reviews accepted the implementation and required
corrections were independently verified in Beads.

| ID | Required behavior and focused proof |
| --- | --- |
| EV-01 | SDL-type dot-call APIs with destination-owned sources check valid scalar/list/nullable/object payloads and reject old field selectors, wrong types, roles, forged sources and caught invalid host calls. Generated and runtime contracts agree without a public `any` path. |
| EV-02 | Exact and explicit all-topic subscriptions broadcast to compatible same-type destinations and preserve named-type isolation, linearization, commit order, no replay and independent broadcast progress. GraphQL cannot select all topics through this API. |
| EV-03 | Sequence starts after one interval, advances without consumer demand, loops or ends correctly and gives independent positions to two subscriptions. Test monotonic time independently of fixed/timeleap wall time. |
| EV-04 | Delayed ticks skip positions in constant work without a catch-up burst; finite late completion emits the last value once. Empty/holey arrays, invalid intervals, capture bounds and sequence exhaustion fail safely. |
| EV-05 | Explicit registrations produce preserved typed module scaffolds and discoverable references; all enabled sources activate before traffic/ticks, disabled callbacks never execute and startup failure cleans every partial registration. |
| EV-06 | Real named-consumer model writes and child publications commit atomically; later host/body/timeout/cancellation failures roll back that candidate, preserve the publisher and allow subsequent events without retry. |
| EV-07 | Original depth 0 can republish through 1, 2 and 3; depth-4 attempts roll back the whole consumer despite `pcall`, field/topic changes, branching or forged metadata. Depth-3 model-only execution can commit. |
| EV-08 | Per-consumer FIFO and single invocation hold under fan-out; payload/provenance, store/clock freshness and publisher credential/state exclusion remain correct for GraphQL and Luau consumers. No VM, setup context or closure survives source waiting. |
| EV-09 | Count/byte/age/shared OR limits handle equality, overflow and recovery for both source kinds; default failure isolates the consumer, DropOldest evicts only eligible owned items and every allocation remains charged until release. |
| EV-10 | Source and encoded evictions compose into the existing GraphQL counter without double counting; tick skips and shutdown/client loss are excluded, partial writes remain immutable and explicit logs expose no automatic payload/secret data. |
| EV-11 | Disconnect, finite completion, source overflow, Immediate/Graceful shutdown, pending callbacks, restart and cancellation release queues/timers/permits under actual physical ownership with no publication-chain drain. |
| EV-12 | Fresh init/codegen/check/reference/export/bundle and renamed-domain HTTP flows prove the coordinated cutover, reject old/mixed formats and retain adopted-schema authority, sandbox, Rust ABI and default dependency boundaries. |

Implement in four work packages: freeze contracts/types/configuration and
version matrix; deliver shared source capture/queues and the new pub/sub path;
add timer sequences and transactional named consumers including depth control;
then complete workflows, examples and integrated focused acceptance. Each
package preserves the existing request/worker boundaries before the next.

Use small local macOS cases with controlled host time and a small real-clock
smoke case, a minimal typed schema plus a renamed schema, model assertions and
actual loopback GraphQL streams. Cover WebSocket, POST SSE and multipart through
the common path without new protocol policy in adapters. Run affected Mock/CLI
tests and applicable Rust quality/dependency/owned-generation checks. Tests use
normative contracts and fixtures, never Beads states or approval prose.

Track independent reviews and findings in Beads. Product acceptance does
not require remote Linux, a collector/broker, browser/IDE, cross/distribution,
full-product validator or load/fuzz campaigns; report their absence honestly.
The implementation has focused local macOS evidence and independent review.
The validation record preserves actual commands, source identities, unsuccessful
runs, corrections and unexecuted qualification; publication alone is never
product-test evidence.
