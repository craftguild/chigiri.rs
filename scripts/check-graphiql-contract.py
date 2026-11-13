"""Verify GraphiQL default privacy, explicit features, and independent consumer builds."""

import json
from pathlib import Path

from implementation_rule_io import require
import subprocess

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = "tests/fixtures/graphiql-consumer/Cargo.toml"


def cargo(*arguments, success=True):
    result = subprocess.run(["cargo", "+1.95.0", *arguments, "--locked", "--offline",
                             "--manifest-path", MANIFEST], cwd=ROOT,
                            text=True, capture_output=True, timeout=300)
    if (result.returncode == 0) != success:
        raise ValueError(result.stdout + result.stderr)
    return result


def main():
    for enabled in (False, True):
        features = ["--features", "graphiql"] if enabled else []
        metadata = json.loads(cargo("metadata", "--format-version", "1", *features).stdout)
        nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
        active = {node["id"] for node in metadata["resolve"]["nodes"]}
        names = {package["name"]: package for package in metadata["packages"] if package["id"] in active}
        require(("chigiri_graphiql" in names) == enabled)
        for name in ("chigiri", "chigiri_axum", "chigiri_hyper"):
            require(("graphiql" in nodes[names[name]["id"]]["features"]) == enabled)
        require(not {"stylua", "full_moon", "ratatui", "crossterm", "luaur"} & set(names))
        cargo("check", "--lib", "--target-dir", "target/graphiql-consumer", *features)
    failure = cargo("check", "--bin", "publication-probe", "--features", "probe",
                    "--target-dir", "target/graphiql-consumer", success=False)
    require("no method named `graphiql`" in failure.stderr)
    cargo("check", "--all-targets", "--features", "probe,graphiql",
          "--target-dir", "target/graphiql-consumer")
    print("GraphiQL consumer contract passed: disabled by default, enabled explicitly, unavailable through the disabled public API, Rust 1.95 compatible, and independent of Node.js")


if __name__ == "__main__":
    main()
