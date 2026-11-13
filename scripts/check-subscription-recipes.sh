#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_root="$repository_root/tests/fixtures/subscription-recipes/apollo"
mkdir -p "$repository_root/target"
temporary_root="$(mktemp -d "$repository_root/target/subscription-apollo-recipe.XXXXXX")"

cleanup() {
  find "$temporary_root" -mindepth 1 -delete
  rmdir "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

cp "$fixture_root/package.json" "$temporary_root/package.json"
cp "$fixture_root/package-lock.json" "$temporary_root/package-lock.json"
cp "$fixture_root/delivery-metadata-link.mjs" "$temporary_root/delivery-metadata-link.mjs"
cp "$fixture_root/delivery-metadata-link.test.mjs" \
  "$temporary_root/delivery-metadata-link.test.mjs"

echo "Node $(node --version); npm $(npm --version)"
npm ci --ignore-scripts --no-audit --no-fund --prefix "$temporary_root"
node --test "$temporary_root/delivery-metadata-link.test.mjs"
