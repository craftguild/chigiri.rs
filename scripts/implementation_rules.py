"""Retain bounded product ownership checks independently of shared measurement."""

from pathlib import Path
import sys

from generated_inventory import owned_rust
from graphiql_inventory import check as owned_assets


def main():
    repository = Path(__file__).resolve().parent.parent
    try:
        owned = owned_rust(repository)
        assets = owned_assets(repository)
    except (OSError, ValueError, TypeError, KeyError, RecursionError):
        print("Generated Rust/asset ownership check failed; verify manifests, markers, inventories, and path types.",
              file=sys.stderr)
        return 1
    print(f"Generated ownership check passed: {len(owned)} generated files excluded from handwritten ownership, "
          f"{len(assets)} embedded code assets verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
