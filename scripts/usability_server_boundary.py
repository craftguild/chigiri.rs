"""Reject server ownership in product code while retaining real loopback tests."""

import re
import sys
from pathlib import Path

from check_host_boundary import (
    explicit_path_declarations, sanitized_source, source_text, test_source,
)


FORBIDDEN = re.compile(
    r"TcpListener|TcpStream|Server::bind|serve_connection|tokio::signal|ctrl_c|graceful_shutdown"
)


def violations(repository):
    for cached in (source_text, sanitized_source, explicit_path_declarations):
        cached.cache_clear()
    found = []
    for crate in ("core", "facade", "runtime", "http", "hyper", "axum"):
        root = repository / "crates" / crate / "src"
        for path in sorted(root.rglob("*.rs")):
            if test_source(path, root):
                continue
            source = sanitized_source(path)
            for match in FORBIDDEN.finditer(source):
                found.append((path, source.count("\n", 0, match.start()) + 1))
    return found


if __name__ == "__main__":
    repository = Path(__file__).resolve().parent.parent
    found = violations(repository)
    for path, line in found:
        print(f"Product server ownership: {path.relative_to(repository)}:{line}", file=sys.stderr)
    if found:
        raise SystemExit("Framework/runtime/bridge owns the server lifecycle.")
