#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-usability.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

fail() {
  echo "$1" >&2
  exit 1
}

forbid_matches() {
  local message="$1"
  local pattern="$2"
  shift 2
  if rg -n --glob '*.rs' --glob '*.md' --glob '*.toml' "$pattern" "$@"; then
    fail "$message"
  fi
}

expect_compile_failure() {
  local binary="$1"
  local fragment="$2"
  local output="$temporary_root/$binary.log"
  if cargo +1.95.0 check --locked --offline \
    --manifest-path tests/fixtures/usability-negative-surface/Cargo.toml \
    --target-dir target/usability-negative-surface \
    --bin "$binary" >"$output" 2>&1; then
    fail "Negative fixture compiled successfully: $binary."
  fi
  if ! grep -Fq "$fragment" "$output"; then
    sed -n '1,160p' "$output" >&2
    fail "Negative fixture diagnostic is missing required text: $binary / $fragment"
  fi
}

# API-10: core is std-only, listener and shutdown are app ownership.
cargo metadata --locked --offline --no-deps --format-version 1 \
  >"$temporary_root/metadata.json"
python3 scripts/usability_dependencies.py "$temporary_root/metadata.json"
python3 scripts/test-usability-dependencies.py "$temporary_root/metadata.json"

python3 scripts/usability_server_boundary.py
python3 scripts/test-usability-server-boundary.py
rg -q 'TcpListener::bind' examples/books/src/main.rs \
  || fail "books application does not own listener."
rg -q 'signal::ctrl_c' examples/books/src/main.rs \
  || fail "The books application does not document shutdown handling."

# API-06: Confine automatic request-identity initialization to the worker-local build path.
python3 - crates/facade/src <<'PY'
import sys
from pathlib import Path

facade = Path(sys.argv[1])
sources = {
    path.relative_to(facade).as_posix(): path.read_text(encoding="utf-8")
    for path in facade.rglob("*.rs")
}


def validate(current: dict[str, str]) -> None:
    draft_source = current["draft.rs"]
    identity_source = current["endpoint/request_identity.rs"]
    if "request_ids: RequestIdPlan::Automatic" not in draft_source:
        raise ValueError("The draft lacks an automatic safe-baseline plan.")
    if "RandomRequestIdFactory" in draft_source:
        raise ValueError("Draft::new is getting a request ID seed.")
    reference_files = sorted(
        name for name, source in current.items() if "RandomRequestIdFactory" in source
    )
    if reference_files != ["endpoint/request_identity.rs"]:
        raise ValueError("The automatic factory type is referenced outside the worker-local build path.")
    constructors = sum(
        source.count("RandomRequestIdFactory::new()") for source in current.values()
    )
    if constructors != 1 or identity_source.count("RandomRequestIdFactory::new()") != 1:
        raise ValueError("The initialization path must construct the automatic request ID factory exactly once.")
    initializers = sum(
        source.count("request_identity::initialize(") for source in current.values()
    )
    if initializers != 1 or current["endpoint/validation.rs"].count(
        "request_identity::initialize("
    ) != 1:
        raise ValueError("The automatic factory initializer must appear exactly once in final validation.")
    configuration_call = "validation::configuration(&mut draft)?"
    if sum(source.count(configuration_call) for source in current.values()) != 1 or current[
        "endpoint/build.rs"
    ].count(configuration_call) != 1:
        raise ValueError("Final validation must connect exactly once to endpoint build preparation.")
    if "creator_process_id: std::process::id()" not in identity_source:
        raise ValueError("The endpoint request-ID wrapper does not verify the process boundary.")


validate(sources)
mutations = []
changed = sources.copy()
changed["draft.rs"] = changed["draft.rs"].replace(
    "RequestIdPlan::Automatic", "RandomRequestIdFactory::new()", 1
)
mutations.append(changed)
changed = sources.copy()
changed["components.rs"] += "\nRandomRequestIdFactory::new();\n"
mutations.append(changed)
changed = sources.copy()
changed["components.rs"] += (
    "\nuse chigiri_observability::RandomRequestIdFactory as AutomaticIds;\n"
    "fn alias_constructor() { let _ = AutomaticIds::new(); }\n"
)
mutations.append(changed)
changed = sources.copy()
changed["endpoint/validation.rs"] = changed["endpoint/validation.rs"].replace(
    "request_identity::initialize(", "initialize_before_build(", 1
)
changed["endpoint.rs"] += "\nrequest_identity::initialize(plan);\n"
mutations.append(changed)
for destination in (None, "endpoint.rs", "endpoint/build.rs"):
    changed = sources.copy()
    changed["endpoint/build.rs"] = changed["endpoint/build.rs"].replace(
        "validation::configuration(&mut draft)?", "skip_final_validation(&mut draft)?", 1
    )
    if destination is not None:
        changed[destination] += "\nvalidation::configuration(&mut draft)?;\n"
    if destination == "endpoint/build.rs":
        changed[destination] += "\nvalidation::configuration(&mut draft)?;\n"
    mutations.append(changed)
for changed in mutations:
    try:
        validate(changed)
    except ValueError:
        continue
    raise SystemExit("Mutation of the automatic request-ID initialization location was not rejected.")
PY

# INJ-12: Excluding old preset/replicate builders from repository consumer and public façade.
consumer_paths=(
  README.md
  examples
  tests/fixtures/vnext-large-product/consumer
  tests/fixtures/repository-consumer
)
forbid_matches "The repository consumer has an obsolete construction surface." \
  'ProductionPreset|ProductionPresetBuilder|production_defaults\(|GraphQlEndpoint::builder\(|ExecutableSchema::builder\(|chigiri_production' \
  "${consumer_paths[@]}"
example_readmes=()
while IFS= read -r readme; do
  example_readmes+=("$readme")
done < <(find examples -type f -name README.md -print | sort)
[[ "${#example_readmes[@]}" -gt 0 ]] \
  || fail "No example README was found."
forbid_matches "Example docs contain spelling variants of obsolete construction concepts." \
  '(?i)production[[:space:]_-]*(preset|defaults?|endpoint)|graph[[:space:]_-]*ql[[:space:]_-]*endpoint' \
  "${example_readmes[@]}"
books_readme="examples/books/README.md"
grep -Fq 'chigiri::Endpoint::builder' "$books_readme" \
  || fail "The books README does not use the single endpoint builder."
grep -Fq 'automatic safe baseline' "$books_readme" \
  || fail "The books README does not document the automatic safe baseline."
grep -Fq 'application-owned listener' "$books_readme" \
  || fail "The books README does not document the application-owned listener."
grep -Fq 'typed inject' "$books_readme" \
  || fail "The books README does not document typed injection."
builder_count="$(
  rg -U -o 'pub[[:space:]]+((const|async|unsafe)[[:space:]]+)*(extern[[:space:]]+("[^"]+"[[:space:]]+)?)*fn[[:space:]]+builder[[:space:]]*(<|\()' \
    crates/facade/src --glob '*.rs' | wc -l | tr -d ' '
)"
[[ "$builder_count" == "1" ]] \
  || fail "Expected one public endpoint builder input, found $builder_count."
expect_compile_failure production-defaults 'no method named `production_defaults`'
expect_compile_failure endpoint-advanced 'no function or associated item named `advanced`'
expect_compile_failure http-endpoint-builder 'could not find `GraphQlEndpoint` in `chigiri_http`'
expect_compile_failure http-missing-context-builder \
  'cannot find type `MissingContextEndpointBuilder` in crate `chigiri_http`'
expect_compile_failure http-context-builder \
  'cannot find type `EndpointBuilder` in crate `chigiri_http`'
expect_compile_failure root-schema-builder 'unresolved import `chigiri::ExecutableSchema`'
expect_compile_failure pre-context-response-hook 'the method `response_hook` exists for struct'
expect_compile_failure build-error-raw-payload 'expected tuple struct or tuple variant'

# Compile ADR-0017's code directly as an external consumer.
doc_fixture="$temporary_root/adr-0017-consumer"
mkdir -p "$doc_fixture/src"
python3 - docs/adr/0017-typed-component-injection-and-override-semantics.md \
  "$doc_fixture" "$repository_root" <<'PY'
import sys
from pathlib import Path

document = Path(sys.argv[1]).read_text(encoding="utf-8")
fixture = Path(sys.argv[2])
repository = Path(sys.argv[3])
start = "<!-- compile-check: adr-0017-one-graph:start -->"
end = "<!-- compile-check: adr-0017-one-graph:end -->"
section = document.split(start, 1)[1].split(end, 1)[0]
snippet = section.split("```rust\n", 1)[1].split("```", 1)[0]
wrapper = r'''#![allow(dead_code)]
use std::time::Duration;

struct TransportQuotaKey;
struct OperationQuotaKey;

impl chigiri::rate::TransportQuotaExtractor for TransportQuotaKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        unimplemented!()
    }
}

impl chigiri::rate::OperationQuotaExtractor<()> for OperationQuotaKey {
    fn extract(
        &self,
        _: &chigiri::OperationContext<'_, ()>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::GraphQlError> {
        unimplemented!()
    }
}

fn compile<I, S, O>(
    resolvers: I,
    source: S,
    observer: O,
) -> Result<chigiri::Endpoint<()>, Box<dyn std::error::Error>>
where
    I: chigiri::IntoEndpointInput<()>,
    S: chigiri::spi::OperationSourceResolver,
    O: chigiri::ErrorObserver,
{
__SNIPPET__
    Ok(endpoint)
}
'''
source = wrapper.replace("__SNIPPET__", snippet)
(fixture / "src/lib.rs").write_text(source, encoding="utf-8")
mutation = source.replace(
    ".body_limit(2 * 1024 * 1024)",
    ".body_limit(ByteLimit::mib(2))",
    1,
)
if mutation == source:
    raise SystemExit("ADR-0017 mutation target is missing.")
(fixture / "mutated.rs").write_text(mutation, encoding="utf-8")
(fixture / "Cargo.toml").write_text(
    f'''[package]
name = "adr-0017-consumer"
version = "0.0.0"
edition = "2024"
rust-version = "1.95"
publish = false

[dependencies]
chigiri = {{ path = "{repository / 'crates/facade'}" }}

[workspace]
''',
    encoding="utf-8",
)
PY
cargo +1.95.0 generate-lockfile --offline --manifest-path "$doc_fixture/Cargo.toml" >/dev/null
cargo +1.95.0 check --locked --offline --manifest-path "$doc_fixture/Cargo.toml" \
  --target-dir target/usability-doc-surface >/dev/null
cp "$doc_fixture/mutated.rs" "$doc_fixture/src/lib.rs"
if cargo +1.95.0 check --locked --offline --manifest-path "$doc_fixture/Cargo.toml" \
  --target-dir target/usability-doc-surface >"$doc_fixture/mutation.log" 2>&1; then
  fail "ADR-0017 API mutation was not rejected."
fi
grep -Fq 'ByteLimit' "$doc_fixture/mutation.log" \
  || fail "ADR-0017 API-mutation diagnostics are incomplete."

# API-10/11: Unifying the HTTP ownership of current normative documents to the façade.
ownership_documents=(
  docs/README.md
  docs/architecture.md
  docs/adr/0004-http-adapters.md
  docs/adr/0008-production-platform-and-developer-workflow.md
  docs/adr/0009-production-request-lifecycle.md
  docs/adr/0016-single-endpoint-builder-and-safe-baseline.md
  docs/implementation-handoff.md
)
if rg -n 'chigiri_http::GraphQlEndpoint|GraphQlEndpoint::handle|chigiri_http endpoint|Chigiri HTTP endpoint' \
  "${ownership_documents[@]}"; then
  fail "The current normative documents are re-integrating the old HTTP endpoint ownership."
fi
for document in "${ownership_documents[@]}"; do
  grep -Fq 'chigiri::Endpoint' "$document" \
    || fail "The current normative document has no single façade ownership: $document."
done
grep -Fq 'façade-private transport pipeline' docs/architecture.md \
  || fail "The architecture has no transport phase owner."
grep -Fq 'façade-private transport pipeline' docs/adr/0004-http-adapters.md \
  || fail "ADR-0004 has no transport phase owner."
grep -Fq 'Generated ABI target: 7' docs/adr/0053-trait-only-resolvers-and-application-boundaries.md \
  || fail "ADR-0053 does not declare generated ABI target 7."
if rg -n 'Generated ABI target: 4|repository.*ABI 4|Accepted public contract.*ABI 4|generated ABI.*3.*4|ABI 3/4.*mix' \
  docs/adr/0008-production-platform-and-developer-workflow.md \
  docs/production-implementation-goal-prompt.md; then
  fail "Obsolete ABI 4 instructions reappeared in the current workflow document."
fi

# Compile the ADR-0019 consumer directly against the current MSRV public surface.
./scripts/check-usability-adr-0019.sh

# Retained PROJ-06/13 invariants now use complete ABI 8 traits and adapters.
if rg -n 'std::collections|std::any::Any|downcast|HashMap|BTreeMap|missing_resolver|partial_resolver|null_fallback' \
  crates/codegen/src/render/dispatch crates/codegen/src/render/dispatch_group; then
  fail "The resolver generator contains a runtime registry or fallback."
fi
grep -Fq 'pub const GENERATED_ABI_VERSION: u32 = 8;' crates/runtime/src/generated.rs \
  || fail "The runtime generated ABI is not 8."
bash scripts/python-with-toml.sh scripts/check-generated-surface.py
expect_compile_failure abi4-generated 'regenerate the schema'

# LIMIT-16: distinct limiter algorithms remain typed without the legacy policy shim.
if rg -n '\b(pub )?(struct|enum|type) RateLimitPolicy\b' crates/facade/src crates/policy/src; then
  fail "The obsolete RateLimitPolicy shim is still present."
fi
expect_compile_failure rate-invalid-combination 'no method named `window`'
expect_compile_failure rate-decision-crossed 'no associated item named `Allowed`'

echo "Developer usability static contract checks passed."
