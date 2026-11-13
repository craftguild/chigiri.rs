#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/chigiri-adr-0019.XXXXXX")"
cleanup() {
  rm -rf -- "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

fail() {
  echo "$1" >&2
  exit 1
}

fixture="$temporary_root/consumer"
mkdir -p "$fixture/src"
python3 - docs/adr/0019-official-rate-limiters-and-provider-spi.md \
  "$fixture" "$repository_root" <<'PY'
import sys
from pathlib import Path

document = Path(sys.argv[1]).read_text(encoding="utf-8")
fixture = Path(sys.argv[2])
repository = Path(sys.argv[3])
start = "<!-- compile-check: adr-0019-official-rate:start -->"
end = "<!-- compile-check: adr-0019-official-rate:end -->"
section = document.split(start, 1)[1].split(end, 1)[0]
snippet = section.split("```rust\n", 1)[1].split("```", 1)[0]
wrapper = r'''#![allow(dead_code)]
use std::time::Duration;

struct TransportQuotaKey;

impl TransportQuotaKey {
    fn new() -> Self {
        Self
    }
}

impl chigiri::rate::TransportQuotaExtractor for TransportQuotaKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        unimplemented!()
    }
}

struct OperationQuotaKey;

impl OperationQuotaKey {
    fn new() -> Self {
        Self
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

fn compile<I>(
    resolvers: I,
) -> Result<chigiri::Endpoint<()>, Box<dyn std::error::Error>>
where
    I: chigiri::IntoEndpointInput<()>,
{
__SNIPPET__
    Ok(endpoint)
}
'''
source = wrapper.replace("__SNIPPET__", snippet)
(fixture / "src/lib.rs").write_text(source, encoding="utf-8")
missing_context = source.replace("    .without_context()\n", "", 1)
if missing_context == source:
    raise SystemExit("ADR-0019 context mutation target is missing.")
(fixture / "missing-context.rs").write_text(missing_context, encoding="utf-8")
old_api = source.replace(
    "    .without_context()",
    "    .production_defaults()\n    .without_context()",
    1,
)
if old_api == source:
    raise SystemExit("ADR-0019 legacy API mutation target is missing.")
(fixture / "old-api.rs").write_text(old_api, encoding="utf-8")
(fixture / "Cargo.toml").write_text(
    f'''[package]
name = "adr-0019-consumer"
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

cargo +1.95.0 generate-lockfile --offline --manifest-path "$fixture/Cargo.toml" >/dev/null
cargo +1.95.0 check --locked --offline --manifest-path "$fixture/Cargo.toml" \
  --target-dir target/usability-adr-0019 >/dev/null

cp "$fixture/missing-context.rs" "$fixture/src/lib.rs"
if cargo +1.95.0 check --locked --offline --manifest-path "$fixture/Cargo.toml" \
  --target-dir target/usability-adr-0019 >"$fixture/missing-context.log" 2>&1; then
  fail "ADR-0019 context-shift mutation was not rejected."
fi
grep -Fq 'no method named `build`' "$fixture/missing-context.log" \
  || fail "ADR-0019 context-transition diagnostics are incomplete."
grep -Fq 'MissingContextEndpointBuilder' "$fixture/missing-context.log" \
  || fail "ADR-0019 context-type diagnostics are incomplete."

cp "$fixture/old-api.rs" "$fixture/src/lib.rs"
if cargo +1.95.0 check --locked --offline --manifest-path "$fixture/Cargo.toml" \
  --target-dir target/usability-adr-0019 >"$fixture/old-api.log" 2>&1; then
  fail "ADR-0019 legacy API mutation was not rejected."
fi
grep -Fq 'no method named `production_defaults`' "$fixture/old-api.log" \
  || fail "ADR-0019 obsolete-API mutation diagnostic is missing."

echo "ADR-0019 contract passed: the MSRV consumer compiles and obsolete API mutations fail as expected."
