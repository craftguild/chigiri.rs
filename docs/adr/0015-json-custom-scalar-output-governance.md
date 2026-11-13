# ADR-0015: Accept JSON-container custom-scalar output under resource limits

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-29
- Last updated: 2026-08-29
- Decision owners: Pythia maintainers
- Depends on: ADR-0005, ADR-0006, ADR-0011, ADR-0012
- Scope: `pythia_core` output completion, execution limits, and HTTP response governance

## 1. Context

Pythia's public `Scalar` trait delegates application-specific scalar input to `Scalar::parse(Value)` and output to `Scalar::serialize(&self) -> FieldResult<Value>`. Core `Value` represents every JSON value: null, boolean, integer, float, string, list, and object.

The previous custom-scalar result coercion accepted only primitives and rejected `Value::Null`, `Value::List`, and `Value::Object` as internal errors. Legitimate JSON/JSONObject scalars therefore could not serialize objects or lists. An object-valued custom scalar exposed this mismatch.

Merely accepting object/list values would let custom scalars bypass existing GraphQL list-item and request-wide output-node limits. Recursive validation would also introduce a stack-overflow path. Accepted value shapes and resource governance must change together.

## 2. Finding

### SCALAR-OUTPUT-01: Public custom-scalar and executor-completion contracts disagreed

- Severity: Medium
- Integration impact: Blocker
- Security impact: the old implementation failed closed, but unbounded containers would enable resource exhaustion
- Status: Resolved
- Affected implementation:
  - `crates/core/src/executor/output.rs`
  - `crates/core/src/executor/engine.rs`
  - `crates/core/src/execution_limits.rs`
  - generated-resolver and HTTP tests exercising custom scalars

This is a framework contract defect. Stringifying JSON in the consumer would change scalar semantics, response shape, and client contracts. The fix must not change the schema to `String` or bypass framework completion.

## 3. Decision

### 3.1 Custom scalars may return any JSON-representable `Value`

After `Scalar::serialize`, accept:

- `Null`
- `Boolean`
- `Int`
- canonical `BigInt`
- finite `Float`
- `String`
- recursively composed `List`
- recursively composed `Object`

Preserve object insertion order and list order. Core `Object` already enforces unique keys. Non-finite floats and noncanonical big integers anywhere in the tree remain internal completion errors.

Built-in scalar result coercion is unchanged: `Int`, `Float`, `String`, `Boolean`, and `ID` still reject object/list/null values.

### 3.2 Add an output-specific depth limit

Expose:

```rust
pub max_custom_scalar_output_depth: usize
```

- `Default` is 128.
- `unbounded()` uses `usize::MAX`.
- The serialized root is depth 0; each container-child edge increments depth.
- Depth equal to the limit is accepted; limit + 1 is rejected.
- A primitive or empty container completes at depth 0.

Do not reuse parser/input `max_parser_nesting`: input syntax and application output are separate trust boundaries. Because this project is unreleased, adding the public field needs no compatibility shim.

### 3.3 Charge nested containers to the existing output budget

The selection field already consumes one output node before resolver invocation, so do not count the custom-scalar root twice. Charge every descendant object-member value and list item as one `max_output_nodes` node.

- Object keys do not count.
- Primitive roots and empty root containers consume no additional node.
- A nested container value counts once, and its children follow the same rule.
- Every field, GraphQL list item, and custom-scalar descendant in a request shares one atomic counter.
- Concurrent sibling completion must not oversubscribe the limit.
- Addition or comparison overflow fails closed.
- A failed completion does not refund previously reserved request budget, matching existing completion semantics.

Add checked multi-node reservation to `Engine` and implement one-node reservation with the same primitive. Stop validation as soon as excess is known rather than counting an entire giant value first:

1. Iteratively validate depth, every list length, and nested numeric values while checked-counting descendants.
2. Once count exceeds request limits or `usize`, stop traversal and iteratively discard the value.
3. Only a fully valid value makes one atomic batch claim.
4. If a sibling wins the budget race, fail without changing the counter.
5. After a successful claim, return the original owned `Value` without reordering it.

Invalid numeric/depth/list values therefore consume no request budget and leave no partial sibling reservation. Existing successful reservations are not rolled back.

### 3.4 Apply list-item limits to every embedded list

Apply `max_list_items_per_field` to every `Value::List`, including root lists, object members, and lists nested inside list items. Exactly the limit succeeds; limit + 1 becomes an internal error without exposing the resolver value.

### 3.5 Validate and discard without recursion

Use an explicit-stack traversal for depth, list length, node count, and numeric validation. Never recurse on the Rust call stack.

Rejecting a deeply nested owned value through ordinary recursive `Drop` could itself overflow the stack. Generalize the existing iterative input-value disposal into a common helper, or provide an equivalent shared implementation, so invalid custom output is also discarded iteratively. Input and output must not duplicate inconsistent depth definitions or disposal algorithms.

Keep a fast path for primitive scalars without container allocation or traversal.

### 3.6 Use existing error, observation, and null-bubbling paths

Limit excess and invalid numbers become internal completion `FieldError` values.

- Do not expose resolver values, measured node counts, or thresholds to clients.
- Preserve error IDs, `ErrorObserver` sources, GraphQL field paths, and coordinates.
- Direct and HTTP execution use the same masking.
- Nullable positions become null; non-null positions bubble to the nearest nullable parent.
- Preserve the source/path behavior of a `FieldError` returned directly by `Scalar::serialize`.

### 3.7 Enforce exact byte limits at the HTTP boundary

Custom objects/lists use the ordinary JSON writer. HTTP applies `response_byte_limit` to exact escaped UTF-8 bytes, retaining the 8 MiB default and masked-500 behavior.

Do not add an estimated byte counter to direct core execution. The same issue applies to ordinary String results; transport-free core returns an in-memory `Value`, and non-HTTP consumers own limits at their serialization boundary.

## 4. Required implementation changes

1. `crates/core/src/execution_limits.rs`
   - add `max_custom_scalar_output_depth` to `Default`, `unbounded()`, and tests.
2. `crates/core/src/executor/engine.rs`
   - add checked atomic batch claims and route single-node claims through them.
3. `crates/core/src/executor/output/`
   - separate built-in and custom scalar paths;
   - validate custom containers iteratively while preserving order;
   - keep tests in separate modules; and
   - call the request-counter-dependent path only from `Engine::complete_nullable`, leaving enum/built-in coercion independent of `Engine`.
4. value traversal/input coercion
   - share iterative depth checking and disposal with invalid output.
5. façade/HTTP/example tests
   - exercise generated `Scalar::serialize` through direct and HTTP paths;
   - verify byte limits, masking, observation, and null bubbling.
6. documents and trace
   - synchronize this ADR, ADR-0005, architecture, conformance, and review records;
   - do not update generated ABI because resolver signatures do not change; and
   - migrate workspace `ExecutionLimits` struct literals to the new field.

### 4.1 Implementation record

- `crates/core/src/execution_limits.rs` gained `max_custom_scalar_output_depth`, default 128 and unbounded `usize::MAX`.
- `crates/core/src/executor/output/{builtin,custom}.rs` separates pure enum/built-in coercion from explicit-stack custom validation. Only the custom path is called through `Engine::complete_nullable`; it preserves the original owned value and insertion order.
- `crates/core/src/executor/engine.rs` routes one-node reservation through a checked atomic batch primitive. Overflow and sibling races fail closed without modifying the counter.
- `crates/core/src/value/traversal.rs` shares input/output depth validation and iterative disposal, including invalid values 10,001 levels deep.
- Generated ABI 4 and the public `Scalar` signature remain unchanged. The generated fixture under `crates/codegen/tests/{fixtures,golden,compile}/custom-scalar-output` tests direct and neutral HTTP execution from one resolver assembly.

Keep responsibilities in files near 350 lines rather than crowding existing modules. Preserve core's zero normal dependencies; do not add serde/runtime crates for container validation.

## 5. Acceptance tests

| ID | Scenario | Expected |
| --- | --- | --- |
| CSO-01 | custom scalar serializes null, primitives, object-with-list, list-with-object | each exact `Value` succeeds |
| CSO-02 | object insertion and list order | preserved in direct and HTTP responses |
| CSO-03 | built-in scalar returns null/object/list | existing result-coercion error |
| CSO-04 | nested non-finite Float or invalid BigInt | internal error without resolver-value disclosure |
| CSO-05 | depth 0, exactly limit, limit + 1 | first two succeed; last fails |
| CSO-06 | invalid output deeper than 10,000 | rejected/discarded without abort or stack overflow |
| CSO-07 | primitive/empty container versus one member/item | additional node cost 0 versus 1 |
| CSO-08 | nested node count exactly budget and +1 | exact succeeds; +1 fails |
| CSO-09 | root/nested list lengths exactly item limit and +1 | exact succeeds at every level; +1 fails |
| CSO-10 | concurrent sibling custom completion | shared request budget cannot be oversubscribed |
| CSO-11 | invalid nullable/non-null custom output | normal GraphQL null bubbling |
| CSO-12 | direct/HTTP limit and numeric errors | preserve ID, source, path, coordinate, and masking |
| CSO-13 | encoded response including custom JSON at limit and limit - 1 | former succeeds; latter is masked 500 |
| CSO-14 | object/list custom-scalar input | preserve existing `Scalar::parse` contract |
| CSO-15 | `Scalar::serialize` returns `FieldError` | preserve diagnostics and trait/ABI |

CSO-01/02 use end-to-end generated schema/resolver tests, not only a helper. CSO-05 through CSO-10 use small deterministic limits for off-by-one and concurrency. CSO-12/13 exercise direct and neutral HTTP paths and prove raw values/thresholds never enter client bodies or `Debug`.

`docs/spec-conformance.md` Section 11 is the canonical one-to-one trace. `pythia_codegen::generation::custom_scalar_output_contract...` launches 18 independent fixture tests and checks golden drift.

## 6. Definition of Done

```text
./scripts/check-implementation-rules.sh
cargo fmt --all -- --check
cargo test -p pythia_core --all-targets --all-features
cargo test -p pythia --all-targets --all-features
cargo test -p pythia_http --all-targets --all-features
cargo clippy -p pythia_core -p pythia -p pythia_http --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p pythia_core -p pythia -p pythia_http --all-features --no-deps
./scripts/check-vnext-adoption.sh
./scripts/check-production-acceptance-trace.sh
```

Run these under MSRV 1.85.0 and current stable, plus all workspace tests on both toolchains and existing generated-drift, dependency, and golden checks.

- Every `.rs`/`.sh` remains at or below 400 physical lines.
- New or changed comments and rustdoc are English under ADR-0043.
- CSO-01 through CSO-15 trace one-to-one from conformance to real tests.
- Core remains runtime-neutral with zero normal dependencies.
- Generated ABI and public `Scalar` signature do not change.
- No application-specific schema, fixture, or domain name enters the framework.
- Independent Reviews A/B leave zero unresolved findings of every severity.
- The same verification reproduces from an isolated clean clone of final HEAD.

New formal benchmarks, 15-minute load/soak, and long fuzzing are outside this finding's scope. Preserve existing performance evidence and use ordinary CI plus focused regressions.

## 7. Consequences

### Positive

- JSON custom scalars retain normal object/list/null semantics.
- Consumers need no stringification or executor bypass.
- Custom-scalar descendants share the GraphQL response resource budget.
- Deep application output fails safely without recursive stack use.

### Negative

- `ExecutionLimits` gains a public field, requiring changes to struct literals.
- Container scalars pay depth/node/list validation cost.
- Direct-execution byte limits remain the consumer's responsibility.

### Rejected alternatives

- Stringify JSON in an external application: changes schema and client response shape.
- Document custom scalars as primitive-only: contradicts public `Value` and common JSON-scalar use.
- Permit unbounded object/list output: bypasses budgets and stack safety.
- Reuse `max_parser_nesting`: couples separate trust boundaries and operational tuning.
- Estimate serialized bytes in core: differs from transport encoding and conflicts with ordinary String behavior.
