"""Verify current public entry points and reject removed publisher/module paths."""

from pathlib import Path


REMOVED = tuple("scripts/" + name for name in (
    "build-release-artifacts.sh", "check-release-artifacts.sh",
    "check-package-fresh-consumer.sh", "package-consumer-gate.sh",
    "check-packaged-source.sh", "prepare-release-reference.sh",
    "generate-provenance.py", "verify-provenance.py", "verify-release-semantics.py",
    "generate-sbom.py", "test-release-artifact-verifier.py", "check-production-readiness.sh",
)) + (
    "tests/fixtures/package-consumer",
    "crates/codegen/src/output.rs", "crates/codegen/src/output",
    "crates/codegen/src/config/parse.rs", "crates/codegen/src/discovery/source.rs",
    "crates/codegen/tests/golden/full/mod.rs",
    "crates/codegen/tests/golden/custom-scalar-output/mod.rs",
    "examples/books/src/graphql/generated",
    "examples/axum-adoption/src/graphql/generated",
    "crates/testkit/tests/usability/generated",
)


def verify(repository):
    for name in REMOVED:
        path = repository / name
        if path.exists() or path.is_symlink():
            raise ValueError(f"Removed path was re-added: {name}")
    library = (repository / "crates/codegen/src/lib.rs").read_text(encoding="utf-8")
    for symbol in ("BindingConfig", "generate_with_config", "check_with_config",
                   "inspect_developer_schema", "fuzz_parse_config", "mod output;"):
        if symbol in library:
            raise ValueError(f"Removed codegen public surface remains: {symbol}")
    for symbol in ("ResolvedManifest", "generate_contract", "update_contract", "check_contract"):
        if symbol not in library:
            raise ValueError(f"Current codegen public surface is missing: {symbol}.")


if __name__ == "__main__":
    verify(Path(__file__).resolve().parent.parent)
    print("Sibling public-surface and removed publisher/module checks passed.")
