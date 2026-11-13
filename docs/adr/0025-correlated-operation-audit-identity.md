# ADR-0025: Safely propagate request and trace identity to operation audit events

> 2026-09-06 amendment: [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> replaces the parts of this document that make publisher, package, and
> fresh-clone checks completion conditions for new implementation work
> (Accepted / Planned). Existing execution semantics, test facts, and historical
> Passed records remain. See ADR-0026 for the new generated-boundary target.

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-31
- Decision owners: Pythia maintainers
- Depends on: ADR-0010, ADR-0012, ADR-0013
- Generated ABI target: 5 (unchanged)

## 1. Context

Audit mode from ADR-0010 records catalog hits, misses, and provider failures as
security evidence before migration to a safelist. The current
`OperationAuditEvent` contains only document digest, catalog revision, mode, and
outcome. It lacks the request and trace IDs needed to correlate the event safely
with HTTP logs, lifecycle events, and internal errors from the same request.

This gap became visible during large-consumer adoption. There is no public
`OperationAuditEvent::request_id()` or `trace_id()`, so an application must join
request-local state, sink wrappers, or document digests itself. That workaround
can associate identities with the wrong concurrent request and leaks Pythia's
request lifecycle into the application.

Treating request or trace IDs as metric labels, document identity, or principal
identity would violate ADR-0010 and ADR-0012, as would suppressing a security
audit through sampling. Operation-source resolution precedes parsing,
validation, and operation selection, so `OperationIdentity` is not yet known.

This correlation requirement applies to every Audit mode consumer, not only one
product. It belongs in Pythia's general public event contract rather than a
consumer workaround.

## 2. Decision

### 2.1 Reuse the existing identity model

Add one read-only `pythia_runtime::RequestIdentity<'a>` view to
`OperationAuditEvent<'a>`.

Its conceptual public contract is:

```rust
pub struct OperationAuditEvent<'a> {
    identity: RequestIdentity<'a>,
    document_digest: DocumentDigest,
    catalog_revision: Option<&'a str>,
    mode: OperationSourcePolicy,
    outcome: OperationAuditOutcome,
}

impl<'a> OperationAuditEvent<'a> {
    pub const fn identity(&self) -> RequestIdentity<'a>;
}
```

The event does not duplicate `request_id()`, `trace_id()`, `operation()`, or
`sampling()`. Consumers use `event.identity().request_id()` and
`event.identity().trace_id()`. This matches `LifecycleEvent` and `ErrorEvent`
and avoids duplicating future identity extensions in an audit-specific API.

No audit-specific identity type, owned `String`, backend-specific trace context,
or principal/product identity is added. `OperationAuditEvent::new` remains
crate-private; consumers cannot construct inconsistent events through a public
constructor.

### 2.2 Source and lifetime

Identity is built only from `RequestEnvelope::request_telemetry()` on the same
`RequestEnvelope` passed to `PersistedOperationResolver::resolve`.

- When telemetry exists, use `telemetry.identity(None)`.
- For a direct or legacy entry without telemetry, use
  `RequestIdentity::new(None, None, None)`.
- Do not regenerate or reconsider request IDs, trace IDs, or sampling decisions.
- Do not look up identity through thread-local or task-local state, a global map,
  or a document digest.

The event borrows telemetry from the envelope until the `record` future
finishes. Pythia performs no framework-side ID clone or allocation. A sink that
stores the event beyond its lifetime or exports it asynchronously must explicitly
copy the bounded IDs it needs inside `record`. No API permits a detached task to
retain a borrowed reference.

### 2.3 Operation identity is intentionally absent

Operation-source resolution runs before document parsing, validation, and
operation selection. At this phase `identity.operation()` is always `None`.
The existing `event.document_digest()` exposes the document digest.

The design rejects a second delayed audit, a provisional identity derived from
an unvalidated operation name, and early parsing of the raw document. Admission,
metrics, and lifecycle events that need an operation key continue to run in
later phases.

### 2.4 Audit delivery is independent from telemetry sampling

The sampling decision from ADR-0012 controls lifecycle and trace export volume;
it does not control security-audit admission from ADR-0010. Audit mode calls the
sink exactly once even for `SamplingDecision::Drop`. The event retains the
actual decision for sinks to observe, but Pythia never omits the event because
of it.

Each resolve attempt in Audit mode follows this delivery contract:

| Registry result | Audit outcome | Sink call | Request result |
| --- | --- | ---: | --- |
| exact known document | `Hit` | exactly once | continue |
| unknown/mismatch/invalid ID | `Miss` | exactly once | continue |
| provider failure | `ProviderUnavailable` | exactly once | continue |

If a request-envelope error rejects the document before it reaches Audit policy,
no audit event is generated. The sink does not apply implicitly to APQ, Safelist,
IdOnly, or Disabled modes.

### 2.5 Failure and observer correlation

The fail-open contract from ADR-0010 remains:

- Registry failure sends a `ProviderUnavailable` event to the sink and continues
  the request.
- Sink failure also continues the request.
- Registry failure is reported exactly once to `ErrorObserver` as
  `OPERATION_AUDIT_PROVIDER_FAILED`; sink failure is reported exactly once as
  `OPERATION_AUDIT_SINK_FAILED`.
- The corresponding `ErrorEvent` and `OperationAuditEvent` carry the request ID,
  trace ID, and sampling decision from the same envelope.
- Existing error-ID generation, request-ID generation, and observer-failure
  semantics do not change.

Without telemetry, audit and error observation still occur. Both events have
`None` request and trace IDs. Missing identity neither becomes a provider failure
nor a required startup setting.

### 2.6 Redaction and cardinality

`OperationAuditEvent::Debug` does not print identity values. Like the existing
`RequestIdentity::Debug`, it may show only presence and sampling. The event,
errors, `Debug`, and metric labels do not gain:

- raw GraphQL documents, variables, field arguments, or result data;
- authorization, cookies, or arbitrary header values;
- actual request or trace IDs in `Debug`;
- principal, tenant, IP, session, wallet/address, or other product identities;
  or
- registry/provider error sources or catalog contents.

An audit sink can retrieve request and trace IDs as correlation values, but they
are not added to the closed metric labels from ADR-0012. Retention, access
control, and backend field mapping for persisted values are application
responsibilities.

### 2.7 Scope and compatibility

Only the server-side operation-audit public event and source resolver change.
The following remain unchanged:

- document digest, operation key, catalog lookup, and APQ/Safelist/IdOnly
  semantics;
- the object-safe async signature and fail-open policy of `OperationAuditSink`;
- HTTP status, GraphQL response, error code, and response body;
- the `RequestTelemetry`, `RequestIdentity`, `ErrorEvent`, and `LifecycleEvent`
  types;
- schema, resolver traits, generator, and checked-in generated source; and
- crate graph, normal dependencies, features, and backend/runtime ownership.

Because Pythia has not reached a production release, there is no compatibility
shim for the old event layout, deprecated accessor, or feature switch. The
generated contract does not change, so generated ABI remains 5 and schema
digests and generated goldens remain unchanged.

## 3. Implementation mapping

The following minimum ownership is fixed. If implementation requires more
files, split by named responsibility without changing the semantics.

| Responsibility | Expected owner |
| --- | --- |
| borrowed identity field/accessor/redacted `Debug` | `crates/operation/src/audit.rs` |
| identity construction from the envelope and sink delivery | `crates/operation/src/source.rs` |
| hit/miss/provider/sink/sampling/absence/concurrency tests | `crates/operation/tests/` |
| actual HTTP response and default-diagnostic redaction | `crates/testkit/tests/operation_http.rs` |
| closed eight-ID trace and mutation guard | `scripts/check-operation-audit-identity-acceptance-trace.sh` |
| exact acceptance trace | `docs/spec-conformance.md` Section 14 |
| architecture and work gates | `docs/architecture.md` Section 22 and `docs/implementation-handoff.md` Section 14 |
| review evidence | Beads review `lab01-c43.4` |

Because the existing `crates/operation/tests/persisted.rs` exceeds 350 lines,
new acceptance tests are not added there. Split an identity-specific test module
or file without losing coverage while moving existing tests.

## 4. Acceptance matrix

| ID | Expected |
| --- | --- |
| AUDIT-ID-01 | A `Hit` event carries the envelope's exact request ID, trace ID, and sampling decision, while operation is `None`. |
| AUDIT-ID-02 | A `Miss` event carries the same identity and retains document digest, revision, mode, and outcome. |
| AUDIT-ID-03 | On provider failure, a `ProviderUnavailable` event and one `ErrorObserver` event independently carry the envelope's exact identity, and the request continues. |
| AUDIT-ID-04 | On sink failure, one `ErrorObserver` event and the audit event independently carry the envelope's exact identity, and the request continues. |
| AUDIT-ID-05 | Success, provider failure, and sink failure without telemetry continue the request; audit and observer request/trace/operation values are `None`, and sampling defaults to Record. |
| AUDIT-ID-06 | `SamplingDecision::Drop` still calls the sink exactly once, and the event retains `Drop`. |
| AUDIT-ID-07 | Concurrent requests with different identities and digests never cross-wire events, and every resolve attempt is recorded exactly once. |
| AUDIT-ID-08 | A real endpoint with a failing registry or sink returns the expected successful data exactly, while its status, body, headers, event/identity `Debug`, and default diagnostics disclose no ID value, raw document, or provider/sink source. |

All eight IDs remain distinct and keep their names. Each traces one-to-one to a
specific fully qualified test or repository check. Existing OP-01 through OP-09,
OBS-01 through OBS-09, 53 production acceptance IDs, and 69 DX/rate IDs are not
removed or weakened.

## 5. Verification and gates

The normative implementation sequence and gates are in
`docs/implementation-handoff.md` Section 14. At minimum, run the following on
the final candidate:

1. `./scripts/check-implementation-rules.sh`
2. `cargo fmt --all -- --check`
3. All-target tests, Clippy with `-D warnings`, and rustdoc with `-D warnings` for
   `pythia_operation` on MSRV 1.85.0 and current stable.
4. Workspace all-target/all-feature tests, Clippy, and rustdoc on MSRV 1.85.0 and
   current stable.
5. `check-operation-audit-identity-acceptance-trace.sh` for exact
   AUDIT-ID-01 through AUDIT-ID-08 trace plus rejection of one missing,
   duplicate, or unknown ID mutation; existing production/DX acceptance trace;
   generated drift; and dependency boundaries.
6. Existing automated production-readiness, package/source/SBOM/provenance, and
   fresh-consumer gates.
7. Independent Reviews A and B with zero findings at every severity.
8. Reproduce the exact final commit in an isolated fresh clone from the committed
   lockfile and leave both original and clone worktrees clean.

The implementation agent does not run the formal 31-by-100 benchmark, 15-minute
load/soak, or 30-minute fuzz targets. Existing manual Gates P-D and P-E remain
unchanged. If the public-event hot-path change invalidates previous formal
performance evidence, record a reacquisition TODO.

## 6. Consequences

### Positive

- Audit hits, misses, and provider failures can be correlated with HTTP,
  lifecycle, and internal errors using the same request and trace identity.
- Applications no longer need task-local state, global maps, or digest joins
  that can cross-wire concurrent requests.
- Security auditing remains independent of sampling while preserving redaction
  and metric cardinality.
- The existing identity surface is reused without new backend or runtime
  dependencies.

### Negative

- The public layout of `OperationAuditEvent` changes, and sinks must recognize
  the identity accessor.
- A sink that stores events long-term must explicitly create owned copies of
  bounded IDs.
- Operation keys are unavailable during source resolution, so some analysis
  must correlate with later lifecycle events in a second step.

## 7. Rejected alternatives

### Keep request-local or global maps in the consumer

This duplicates the framework lifecycle and can cause cleanup failures or
identity cross-wiring during cancellation, drop, and concurrent requests.

### Add direct request-ID and trace-ID getters to the event

This differs from Lifecycle and Error events and duplicates getters whenever
the identity model grows.

### Delay auditing until operation identity is known

Provider failure during source resolution cannot be carried into the later
phase, and Audit mode responsibilities and failure ordering would change.

### Skip the audit sink when sampling is Drop

This reuses telemetry sampling for security admission and loses miss evidence
needed to decide whether a safelist migration is safe.

### Copy request and trace IDs into owned strings on the event

This adds request-hot-path allocations and differs from the existing borrowed
identity contract.
