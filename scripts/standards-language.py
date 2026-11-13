#!/usr/bin/env python3
"""Execute the product catalog when needed and verify source-bound gate evidence."""

import ast
import json
from pathlib import Path
import subprocess
import sys

from validation_evidence import source_snapshot, validate_report
from validation_plan import plan

ROOT = Path(__file__).resolve().parent.parent


def product_receipt(required):
    marker = json.loads((ROOT / ".craftguild/current-ci.json").read_text())
    if set(marker) != {"schema_version", "profile", "source_sha256", "completion"} \
            or marker["schema_version"] != 1 or marker["profile"] != "ci":
        raise ValueError("Missing current ordinary product validation")
    path = Path(marker["completion"])
    if path.is_absolute() or ".." in path.parts or path.parts[:2] != ("target", "product-validation"):
        raise ValueError("Invalid product completion location")
    report = json.loads((ROOT / path).read_text())
    validate_report("ci", plan("ci"), report)
    if report["source_sha256"] != marker["source_sha256"] \
            or report["source_sha256"] != source_snapshot(ROOT)["sha256"]:
        raise ValueError("Product evidence does not describe the current complete source")
    commands = {command["label"]: command for command in report["commands"]}
    if not set(required) <= commands.keys():
        raise ValueError("Product evidence lacks a required language gate")
    print("Current-source product gates verified: " + ", ".join(required)
          if required else f"Current-source ci profile verified ({len(commands)} ordered commands)")


def ensure_product_receipt():
    try:
        product_receipt([])
        return
    except (OSError, ValueError):
        pass
    subprocess.run(["bash", "scripts/validate.sh", "--profile", "ci"],
                   cwd=ROOT, check=True)
    product_receipt([])


def run(argv):
    subprocess.run(argv, cwd=ROOT, check=True, timeout=1800)


def main(action):
    if action == "test":
        ensure_product_receipt()
    elif action == "format":
        product_receipt(["workspace-format", "cli-mock-format", "graphiql-consumer-format",
                         "full-schema-domain-format", "fuzz-format"])
    elif action == "lint":
        product_receipt(["workspace-clippy-1.95.0", "workspace-doc-1.95.0",
                         "cli-mock-clippy-1.95.0", "cli-mock-doc-1.95.0"])
    elif action == "javascript-types":
        product_receipt(["graphiql-assets-drift"])
    elif action == "tooling-receipt":
        product_receipt(["standards-tooling"])
    elif action == "javascript":
        for name in ("entry.mjs", "build.mjs"):
            run(["node", "--check", str(ROOT / "crates/graphiql/frontend" / name)])
    elif action == "tooling":
        for name in ("scripts/sdk_preflight.py",
                     "scripts/distribution-compiler.py", "scripts/package-distribution.py",
                     "scripts/standards-acceptance.py"):
            ast.parse((ROOT / name).read_text(), filename=name)
    else:
        raise ValueError("Expected test, format, lint, javascript, javascript-types, tooling or tooling-receipt")


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("Expected one language action")
        main(sys.argv[1])
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
