#!/usr/bin/env python3
"""Verify registered SDK ABI declarations and complete trait boundaries."""

import re
from pathlib import Path

from generated_inventory import output_roots, owned_rust
from implementation_rule_io import read_file


repository = Path(__file__).resolve().parent.parent
owned = owned_rust(repository)
roots = output_roots(repository)
if not roots:
    raise SystemExit("No managed generated SDK was found.")

for root in roots:
    for relative, required in (
        ("src/lib.rs", "::chigiri::GENERATED_ABI_VERSION == 8"),
        ("src/__chigiri/mod.rs", "generated ABI version: 8"),
    ):
        source = read_file(repository / root / relative, 64 * 1024 * 1024).decode("utf-8")
        if required not in source:
            raise SystemExit(f"Generated contract does not declare ABI 8: {root / relative}")

resolver_files = []
for path in sorted(owned):
    if "/projection/" in path:
        raise SystemExit(f"Retired projection output: {path}")
    source = read_file(repository / path, 64 * 1024 * 1024).decode("utf-8")
    if any(token in source for token in ("ResolverFuture", "pub mod projection", "pub fn project_")):
        raise SystemExit(f"Retired implementation surface: {path}")
    if "/resolvers/" in path:
        resolver_files.append(path)
        if not re.search(r"pub trait \w+Resolver<C, T, E>", source):
            raise SystemExit(f"Resolver contract lacks its required error parameter: {path}")
        if "::std::error::Error" not in source or "::std::result::Result<" not in source:
            raise SystemExit(f"Resolver contract lacks typed errors: {path}")
    if "/internal/dispatch/" in path and re.search(
        r"std::collections|std::any::Any|downcast|HashMap|BTreeMap|missing_resolver|partial_resolver|null_fallback", source
    ):
        raise SystemExit(f"Generated adapter contains a runtime registry or fallback: {path}")
if not resolver_files:
    raise SystemExit("No complete resolver trait was inspected.")
print(f"Generated surface passed: {len(roots)} SDKs, {len(resolver_files)} resolver files, ABI 8")
