# ADR-0014: Permit reserved markers at response roots under strict differential-allowlist conditions

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0007
- Corrects: ADR-0007 Section 9's unconditional rejection of response-subtree roots
- Scope: `pythia_testkit` differential comparator and allowlist

## 1. Context

ADR-0007 unconditionally rejects pointers such as `/body/data` and `/body/errors` to prevent one allowlist entry from accepting an entire subtree. That ADR also normalizes differences between containers and scalars, missing values, and different container types into reserved `$pythiaDifference` markers plus exact descendant leaf differences.

For example, the comparator emits this exact difference when old `data` is null and new `data` is an empty object:

```json
{
  "pointer": "/body/data",
  "old": null,
  "new": {"$pythiaDifference": "object"}
}
```

The previous validator rejected `/body/data` before checking whether either value was a reserved marker. The allowlist therefore could not express a difference produced by its own comparator. Strict migrations could not accept specification fixes such as nullable-root GraphQL null bubbling without changing baselines.

## 2. Finding

### DIFF-ROOT-01: Comparator output exceeded allowlist expressiveness

- Severity: Medium
- Integration impact: Blocker
- Status: Resolved
- Affected implementation:
  - `crates/testkit/src/differential/compare/value.rs`
  - `crates/testkit/src/differential/allowlist.rs`
  - `examples/axum-adoption/tests/differential/`

Unconditional rejection correctly prevents one `/body/data` entry from accepting every descendant. Reserved markers, however, contain no object or array snapshot, and nonempty containers still expand into structural markers and exact leaves. Rejecting marker entries was therefore stronger than the safety goal and internally inconsistent.

The fix must not alter consumer baselines, schema nullability, or response normalization; duplicate the testkit; exclude cases; or introduce snapshot/wildcard allowlists.

## 3. Decision

### 3.1 Classify pointers into three sets

1. Ordinary exact pointers:
   - `/status`
   - `/mediaType`
   - `/headers/{name}/{canonical index}`
   - `/body/{segment}/{segment...}`, requiring at least two segments after `body`
2. Conditional response-member roots:
   - `/body/data`
   - `/body/errors`
   - `/body/extensions`
3. Always forbidden:
   - `/body`
   - any other `/body/{segment}`
   - pointers with wildcards, trailing slash, empty segments, or invalid escapes

A conditional root is valid only for an entry satisfying Section 3.2. Ordinary values at those roots remain forbidden.

### 3.2 Validate reserved markers across the complete entry

A reserved marker is an object that satisfies all of these rules:

- its only key is `$pythiaDifference`;
- its value is a string; and
- that string is one of `missing`, `object`, or `array`.

At a conditional root, at least one of old/new must be a reserved marker. The other is limited to a JSON scalar, null, or another reserved marker. Raw object/array snapshots, extra marker keys, and unknown marker values remain invalid.

This expresses:

- transitions between null and empty/nonempty objects or arrays;
- object-to-array transitions;
- transitions between a missing response member and a container; and
- exact presence differences between missing and a scalar/null.

The last case is exact because the `missing` marker contains no subtree. Scalar-to-scalar and null-to-scalar differences remain forbidden at response-member roots.

### 3.3 Preserve leaf-by-leaf consumption

Comparator normalization and consumption rules do not change:

- A nonempty container emits its member-root marker and every descendant structural marker and leaf difference.
- One marker entry consumes exactly one marker difference.
- A marker entry never consumes descendants.
- Any missing descendant entry remains an unallowlisted-difference failure.
- Extra marker/leaf entries fail as unused.
- Duplicate entries, unknown cases, and `regression` classifications remain failures.
- An empty container has no descendants, so its member-root marker is the complete exact difference.

### 3.4 Do not permit parser bypasses

Because validity depends on old/new values, validate the whole entry, not just its pointer. Apply the same predicate to JSON-parsed entries and `AllowlistEntry` values built through public Rust APIs. Keep revalidation at `compare_corpus`.

The implementation centralizes this in entry-level validation such as `valid_difference_target(pointer, old, new)`, rather than broadening `valid_difference_pointer(pointer)` unconditionally. Private names may differ, but validation order must not create a bypass.

### 3.5 Do not weaken comparator output

Do not resolve this finding by deleting comparator differences or treating null and object as equal. Preserve `normalized_value`, container-child expansion, array-index differences, and exact volatile-pointer consumption. The primary change belongs in allowlist validation.

## 4. Required implementation changes

1. `crates/testkit/src/differential/allowlist.rs`
   - centralize reserved-marker recognition;
   - validate pointer and old/new together;
   - conditionally permit only the three Section 3.1 roots; and
   - reuse one validation path for parsing and `compare_corpus`.
2. `examples/axum-adoption/tests/differential/`
   - split new responsibility from `policy.rs`, which was already near 350 lines, into `root_markers.rs` or equivalent; and
   - add every Section 5 positive and negative case.
3. `docs/adr/0007-existing-axum-product-adoption.md`
   - replace Section 9's unconditional prohibition with this conditional exception.
4. `docs/spec-conformance.md` and review records
   - trace `DIFF-01` or a successor ID to added tests; and
   - record the capability gap, fix, and unresolved-finding count.

Do not change public types, allowlist JSON fields, marker strings, closed classifications, or generated ABI.

The implementation uses `valid_difference_target` plus a reserved-marker helper in `crates/testkit/src/differential/allowlist.rs`. Both `parse_allowlist` and `compare_corpus` pass through `validate_allowlist_entries`. Comparator normalization, expansion, and consumption are unchanged. Acceptance responsibility is split under `examples/axum-adoption/tests/differential/root_markers.rs`.

## 5. Acceptance tests

| ID | Scenario | Expected |
| --- | --- | --- |
| ROOT-01 | `/body/data`: null → empty object | one marker passes |
| ROOT-02 | `/body/data`: empty object → null | one marker passes |
| ROOT-03 | `/body/data`: null → nonempty object | root marker plus every descendant marker/leaf passes |
| ROOT-04 | ROOT-03 with only the root marker or one descendant omitted | fails |
| ROOT-05 | `/body/data`: object ↔ array | root marker plus every index/descendant difference passes |
| ROOT-06 | `/body/errors`: missing ↔ empty/nonempty array | empty needs only root marker; nonempty needs all descendants |
| ROOT-07 | `/body/extensions`: missing/null ↔ object | root marker plus every existing descendant passes |
| ROOT-08 | `/body/data`: missing ↔ scalar/null | exact marker entry passes |
| ROOT-09 | `/body/data`: scalar ↔ scalar or null ↔ scalar | parser and direct construction both fail |
| ROOT-10 | reserved marker at `/body` | fails |
| ROOT-11 | reserved marker at `/body/custom` | fails |
| ROOT-12 | raw object/array snapshot at a conditional root | fails |
| ROOT-13 | unknown marker, extra key, wildcard, or invalid escape | fails |
| ROOT-14 | extra marker/leaf for a nonempty container | fails as unused |
| ROOT-15 | duplicate entry, unknown case, or `regression` classification | fails |

ROOT-01 through ROOT-08 run through `parse_allowlist`, with at least one also using a directly constructed `AllowlistEntry`. ROOT-09 through ROOT-13 fail through both the parser and `compare_corpus`, proving that the parser cannot be bypassed.

## 6. Definition of Done

```text
./scripts/check-implementation-rules.sh
cargo fmt --all -- --check
cargo test -p pythia_testkit --all-targets --all-features
cargo test -p pythia_axum_adoption --test differential
cargo clippy -p pythia_testkit -p pythia_axum_adoption --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p pythia_testkit -p pythia_axum_adoption --all-features --no-deps
./scripts/check-vnext-adoption.sh
```

- Every `.rs` and `.sh` remains at or below 400 physical lines.
- New comments and rustdoc are English under ADR-0043.
- ROOT-01 through ROOT-15 trace to real tests.
- Existing differential negative tests pass.
- The minimal `null → object marker at /body/data` reproduction consumes the allowlist without changing the baseline.
- No application-specific schema, case ID, or response snapshot enters the Pythia repository.
- Independent review has zero unresolved Critical, High, Medium, or Low findings.

The 2026-08-29 implementation, verification, and Reviews A/B are recorded in Beads review `lab01-c43.9`. Relevant MSRV/stable tests, Clippy, rustdoc, workspace tests, the implementation checker, vNext adoption, production acceptance trace, formatting, and `git diff --check` all passed with zero unresolved findings.

## 7. Consequences

### Positive

- Strict allowlists can represent structural differences produced at response-member roots.
- GraphQL specification fixes can be introduced without baseline replacement or weaker comparison.
- Empty-container transitions are accepted exactly.

### Negative

- Pointer validity now depends on old/new values as well as the pointer.
- Response-root negative regressions require ongoing maintenance.

### Rejected alternatives

- Unconditionally permit `/body/data`: reintroduces subtree-wide acceptance.
- Remove root markers from comparator output: loses container-kind and empty-container differences.
- Treat null and an empty object as equal: hides a real GraphQL response difference.
- Duplicate the testkit only in an external application: transfers a framework inconsistency to a consumer.
