# ADR-0010: Standardize server-side operation identity and optional persisted-operation policy

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-27
- Decision owners: Pythia maintainers
- Depends on: ADR-0009
- Revisits: ADR-0006

## 1. Context

In production, raw query text alone is not a stable key for safelists, quotas, low-cardinality metrics, client rollout, or schema-impact analysis. APQ, trusted documents, and operation safelists look similar, but dynamic APQ registration reduces bandwidth and parse cost; it is not a security policy that rejects unknown operations.

Client operation placement, result/variable types, query embedding, SDKs, and persisted-ID generation belong to client build tooling. Generating them in Pythia would couple a Rust server framework to client languages and release cycles.

The server must still identify received operations stably, resolve optional persisted IDs, and apply APQ and safelist policies as distinct security modes. That server-side capability does not require Pythia to provide client codegen.

## 2. Decision

### 2.1 Client tooling is out of scope

Pythia does not provide:

- filesystem discovery or watching for operation documents;
- client result/variable types or typed document nodes;
- client SDKs, query embedding, or transport wrappers;
- client-facing operation-ID or manifest generation commands; or
- a client artifact format or package layout.

`cargo pythia generate/check` continues to generate only server contracts from SDL. Running Pythia requires no client repository, operation-document directory, or particular client tool such as The Guild.

Applications that use persisted operations convert output from their chosen tooling into server-side `OperationRegistry`/`OperationCatalog` contracts. Applications own adapters, build pipelines, and distribution formats; Pythia owns validation, lookup, and admission after entries cross that boundary.

Operation-key calculation and lifecycle exposure are framework invariants used even for ordinary queries. APQ, Audit, Safelist, IdOnly, registries, and catalogs are opt-in capabilities. Production-readiness gates validate those capabilities without requiring every application to adopt persisted operations.

### 2.2 Identity model

| Name | Definition | Purpose |
| --- | --- | --- |
| schema digest | canonical SDL digest produced by the server generator | deployment/schema version |
| document digest | lowercase SHA-256 of exact document UTF-8 bytes | server identity and integrity |
| operation key | document digest plus operation name or single-operation sentinel | policy and metrics |
| persisted operation ID | opaque bounded ID defined by the application registry | transport lookup |

Do not normalize whitespace, fragment order, or field order before computing a document digest. Semantically equivalent documents with different transport bytes have different digests. A persisted ID may equal the digest or be an alias, but one registry ID maps to exactly one document.

Multi-operation documents require `operationName`, which is part of the key. A named single operation uses its explicit name; an anonymous operation uses a fixed sentinel. Operation names alone are never global IDs.

### 2.3 Registry and catalog

Separate runtime lookup from offline validation.

| Contract | Required behavior |
| --- | --- |
| `OperationRegistry` | asynchronously look up the same catalog entry by persisted ID or exact document digest |
| `OperationCatalog` | enumerate deployment-approved entries for startup/CI schema validation |

Each catalog entry contains a persisted ID, exact document, and optional bounded cost hint as static metadata. Pythia parses and validates every entry and rejects duplicate IDs, different documents under one ID, unknown operations, and invalid documents. After validation it computes the document digest, operation key, kind/name, and complexity summary as server metadata.

Pythia does not prescribe a JSON, YAML, database, or client-package source format. Applications may build catalogs/registries with Rust builders, build scripts, or custom loaders/providers. The standard implementation is only a bounded immutable in-memory catalog populated by application entries.

Catalogs are optional. A server without persisted operations computes operation keys from raw queries and configures no registry/catalog. If configured, catalog revision/digest may appear in readiness metadata separately from schema digest.

### 2.4 HTTP request envelope

Decoded `pythia_http` input uses a bounded envelope that can represent unresolved source:

```text
document text: present / absent
persisted ID: present / absent
operationName
variables
extensions
```

ADR-0009 phase 5 resolves the envelope into a complete `GraphQlRequest`. Core parse/execution always receives document text; optional-query behavior does not spread through the executor.

The built-in extractor supports Apollo APQ `extensions.persistedQuery.version = 1` and `sha256Hash`. The hash must already be exactly 64 lowercase hexadecimal characters; malformed values are request errors. When query and hash are both present, verify SHA-256 over exact query bytes.

### 2.5 Modes

| Mode | Query text | Known ID | Unknown ID | Dynamic write |
| --- | --- | --- | --- | --- |
| Disabled | required | reject ID as unsupported | n/a | no |
| APQ | allowed | lookup | register only with text and matching hash | yes |
| Audit | allowed | record | continue and record miss | no |
| Safelist | optional | allow only catalog match | reject | no |
| IdOnly | forbidden | resolve from registry | reject | no |

Never describe or configure APQ as a safelist. APQ dynamic stores and Safelist/IdOnly registry/catalog contracts use separate types and builder methods.

Safelist permits only an explicit ID or a document digest computed from query text that exists in the catalog. If both ID and query are present, require agreement among ID, exact query bytes, and catalog entry. IdOnly requires an explicit ID and forbids query text.

Audit is a migration mode and permits unknown operations; production gates never treat Audit as completed safelisting.

### 2.6 Store/provider boundary

The runtime-neutral provider SPI represents:

- immutable registry/catalog lookup;
- bounded APQ get/put;
- an optional audit-event sink;
- provider timeout/cancellation; and
- document-byte and entry-count limits.

Do not add Redis, Memcached, filesystem, or remote-registry SDKs to `pythia_core`. Standard implementations are an immutable in-memory catalog and bounded in-memory APQ cache populated by applications. Multi-node APQ consistency belongs to the provider. IdOnly/Safelist readiness verifies that every instance uses the same catalog revision.

Failure behavior is fixed by mode:

- APQ lookup failure: masked 503; never fall back to accompanying query text.
- Audit sink failure: continue the request and send a failure event to the local observer.
- Safelist/IdOnly lookup failure: fail closed with masked 503; no fail-open setting.

### 2.7 Error contract

Reserve these framework codes:

- `PERSISTED_OPERATION_NOT_FOUND`
- `PERSISTED_OPERATION_HASH_MISMATCH`
- `PERSISTED_OPERATION_NOT_ALLOWED`
- `PERSISTED_OPERATION_NOT_SUPPORTED`
- `PERSISTED_OPERATION_UNAVAILABLE`

Unknown IDs and safelist misses reveal no candidate IDs, documents, or catalog contents. APQ not-found alone may be a client-safe protocol response asking the client to resend. IdOnly and Safelist misses use one public message to avoid revealing membership. Provider failures use 503 and `PERSISTED_OPERATION_UNAVAILABLE` without provider identity or source.

### 2.8 Observability and rate-limit handoff

After source resolution, pass the operation key to subsequent ADR-0009 contexts. Never use raw query text as a metric label. ADR-0011 may use the key and validated catalog complexity metadata for quota keys/cost; ADR-0012 may record schema digest, catalog revision, mode, and hit/miss in bounded events.

### 2.9 Accepted Rust contract and exact HTTP result

Framework-invariant `SchemaDigest`, `DocumentDigest`, `OperationName`, `OperationKey`, and `OperationIdentity` are std-only `pythia_core` value types. The `pythia` façade computes digests through `sha2`; core contains no hashing implementation. Operation names are GraphQL Names or the anonymous sentinel and use at most 255 owned bytes.

Opt-in `pythia_operation` exposes:

- `PersistedOperationId`: 1–256 bytes of visible ASCII, rejecting whitespace/controls; `Debug` redacts the value;
- `OperationRegistry`: object-safe asynchronous `lookup`, returning exact document or not-found;
- `OperationCatalog`: synchronous offline enumeration of a bounded entry slice and revision;
- `InMemoryOperationCatalog` and `InMemoryApqStore`: build-time entry/document byte limits; and
- `OperationSourcePolicy`: closed `Disabled`, `Apq`, `Audit`, `Safelist`, and `IdOnly` enum.

The `pythia_http::OperationSourceResolver` bridge owns provider-future timeout/drop. Source results are:

| Result | HTTP status | Code |
| --- | ---: | --- |
| APQ unknown ID | 200 | `PERSISTED_OPERATION_NOT_FOUND` |
| hash mismatch / malformed persisted envelope | 400 | `PERSISTED_OPERATION_HASH_MISMATCH` / `BAD_REQUEST` |
| ID supplied while Disabled | 400 | `PERSISTED_OPERATION_NOT_SUPPORTED` |
| Safelist/IdOnly miss or three-way mismatch | 403 | `PERSISTED_OPERATION_NOT_ALLOWED` |
| provider timeout/failure | 503 | `PERSISTED_OPERATION_UNAVAILABLE` |

Preserve negotiated media in every case. Only APQ not-found may use a message that distinguishes catalog membership.

## 3. Compatibility and rollout

Use this migration order:

1. Build an adapter from chosen client/application tooling into a server catalog.
2. Validate every catalog entry against the schema in CI while remaining Disabled.
3. Observe unknown operations in Audit.
4. Permit only known operations in Safelist.
5. Move clients that implement an ID-only protocol to IdOnly.

APQ is a separate work package from this security migration. The Audit-to-Safelist release evidence records observation duration, zero unknown operations, and owner approval.

## 4. Alternatives considered

### Operation name alone

Different documents/clients can share a name, and selection changes would be invisible.

### Canonically reprint then hash at runtime

This creates a new compatibility boundary around printer versions and semantics-preserving transformations and differs from standard exact-byte APQ hashing.

### Generate operation types in the server resolver generator

This couples server-SDL and client-document artifacts/ABIs and requires unnecessary input for servers that do not manage operations.

### Require a Pythia-specific client manifest

This fixes client languages, tooling, and deployment pipelines. Applications instead adapt arbitrary sources to server-side registry/catalog contracts.

## 5. Consequences

### Positive

- Safelists, metrics, quotas, and schema-impact checks share one operation key.
- APQ cannot be mistaken for a security allowlist.
- Applications can connect any client tooling through adapters.
- Server resolver codegen remains simple.

### Negative

- Persisted-operation users must own registry/catalog adapters and distribution.
- Client and server builds must agree on exact document bytes.
- HTTP decoding now produces an envelope before source resolution.

## 6. Acceptance matrix

| ID | Expected evidence |
| --- | --- |
| OP-01 | Identical document bytes always have one digest; a one-byte difference changes it. |
| OP-02 | A multi-operation document without `operationName` is rejected, and keys differ by name. |
| OP-03 | Catalog validation rejects invalid/unknown entries, duplicate IDs, and schema mismatch. |
| OP-04 | APQ hash mismatch is rejected before store writes. |
| OP-05 | APQ and Safelist unknown-ID behavior differs as specified, and both opaque aliases and raw document digests reconcile correctly. |
| OP-06 | IdOnly rejects query text and resolves a known ID to the registry's exact document. |
| OP-07 | Provider timeout/failure follows mode-specific failure policy and drops futures. |
| OP-08 | Audit records digest/revision/mode/hit-miss without exposing raw query, variables, or catalog contents in errors or metric labels. |
| OP-09 | A neutral endpoint and application-owned-server quick start succeeds without Pythia client tooling, client manifests, or operation directories. |

Before acceptance, fix Apollo APQ request/response fixtures, registry/catalog provider traits, a static catalog builder, and an application-owned adapter compile skeleton. Client artifact schemas are not acceptance scope.
