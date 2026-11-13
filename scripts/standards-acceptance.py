#!/usr/bin/env python3
"""Run the CLI's Rust acceptance test against the verified built artifact."""

import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def verify():
    subprocess.run(["sh", "tools/standards.sh", "verify-receipt",
                    "--artifact", "chigiri-cli"], cwd=ROOT, check=True)


def main():
    verify()
    receipt_path = ROOT / ".craftguild/chigiri/receipt.json"
    receipt = json.loads(receipt_path.read_text())
    context = receipt["context"]
    environment = dict(os.environ, CHIGIRI_ACCEPTANCE_RECEIPT=str(receipt_path),
                       CRAFTGUILD_COMMAND=context["command"],
                       CRAFTGUILD_PREPARED=receipt["environment"]["CRAFTGUILD_PREPARED"])
    profile = "dev" if context["profile"] == "debug" else context["profile"]
    subprocess.run([
        "cargo", f"+{context['toolchain']}", "test", "--locked", "-p", "chigiri_cli",
        "--test", "artifact_acceptance", "--target", context["target"],
        "--profile", profile, "--target-dir",
        str(Path(context["target_dir"]) / "acceptance-tests"), "--", "--ignored",
    ], cwd=ROOT, env=environment, check=True)
    verify()


if __name__ == "__main__":
    main()
