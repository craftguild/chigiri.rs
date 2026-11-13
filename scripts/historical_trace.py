"""Load a frozen historical acceptance trace used as a checker baseline."""

import re
import sys
from pathlib import Path


def load_trace(repository: Path, key: str) -> str:
    if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", key):
        raise ValueError("Invalid historical trace key.")
    path = repository / "scripts/fixtures/historical-acceptance-traces" / f"{key}.md"
    return path.read_text(encoding="utf-8")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: historical_trace.py TRACE")
    try:
        sys.stdout.write(load_trace(Path(__file__).resolve().parent.parent, sys.argv[1]))
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from error
