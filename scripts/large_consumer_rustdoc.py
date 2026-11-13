"""Check the current public large-consumer docs without accepting missing inputs."""

import re
import sys
from pathlib import Path

from implementation_rule_io import read_file


REQUIRED = frozenset({
    "index.html", "struct.Resolvers.html", "trait.SchemaTypes.html",
    "trait.QueryResolver.html", "trait.MutationResolver.html",
    "trait.ProductObject000Resolver.html",
})
PRIVATE = re.compile(
    r"__Chigiri|ProjectionSlots|ProjectionPlan|ResolverStage|"
    r"ResolverDispatch|schema_fingerprint|ArgumentValues"
)
HTML_PRIVATE = re.compile(
    r"__Chigiri|ProjectionSlots|ProjectionPlan|ResolverDispatch|schema_fingerprint|ArgumentValues"
)


def validate(documentation):
    public = documentation / "chigiri_large_contract"
    html = sorted(public.rglob("*.html"))
    if not REQUIRED.issubset({path.relative_to(public).as_posix() for path in html}):
        raise ValueError("Required public large-consumer rustdoc pages are missing.")
    projection = public / "projection"
    if projection.exists() or projection.is_symlink():
        raise ValueError("The retired projection API appears in public rustdoc.")
    indexes = sorted(path for path in documentation.rglob("*.js")
                     if path.name.startswith("search-index") or "search.index" in path.parts)
    if not indexes:
        raise ValueError("The rustdoc search index is missing.")
    for path in [*html, *indexes]:
        source = read_file(path, 64 * 1024 * 1024).decode("utf-8")
        # A hidden assembly stage can appear as an opaque return type, but
        # must have no public item page or search entry (ADR-0053 / RI-08).
        pattern = HTML_PRIVATE if path.suffix == ".html" else PRIVATE
        if pattern.search(source) or PRIVATE.search(path.name):
            raise ValueError(f"Public rustdoc exposes a private helper: {path.name}")


if __name__ == "__main__":
    try:
        validate(Path(sys.argv[1]))
    except (OSError, ValueError) as error:
        raise SystemExit(f"Large-consumer rustdoc check failed: {error}") from error
    print("Large-consumer public rustdoc and search indexes hide private helpers.")
