# Pinned third-party sources for Chigiri Mock

This directory contains the crates.io sources for StyLua 2.5.2 and full_moon 2.2.0.
The formatter/parser MPL-2.0 licenses are included. Rust source files remain
byte-for-byte identical to the published archives. Limited `Cargo.toml` changes
are checked against originals stored as `Cargo.toml.upstream`.

- StyLua omits unused CLI, benchmark, test targets, and CLI-only dependencies.
- full_moon replaces the unmaintained `paste` package with the compatible
  `pastey = 0.2.3` dependency alias.
- No advisory suppression is added. The inventory covers retained source files,
  including files that the build does not compile.

Versions, archive digests, and file digests are recorded in
[`scripts/vendored-sources.json`](../scripts/vendored-sources.json). The verifier
rejects unregistered files and source changes. Normal Cargo builds use these local
sources without launching external formatters or downloads.

```sh
bash scripts/python-with-toml.sh scripts/vendored_sources.py
# Reapply the limited manifest patch after checking original hashes.
bash scripts/python-with-toml.sh scripts/vendored_sources.py --refresh
```

Distributions include the patched manifests, sources, verifier, inventory,
licenses, and this notice. Upstream sources:

- [StyLua 2.5.2](https://github.com/JohnnyMorganz/StyLua/tree/v2.5.2)
- [full_moon 2.2.0 distribution source](https://crates.io/api/v1/crates/full_moon/2.2.0/download)
- [full_moon_derive 0.11.0 distribution source](https://crates.io/api/v1/crates/full_moon_derive/0.11.0/download)
- [Bundled MPL-2.0 text](stylua-2.5.2/LICENSE.md)
- [Official MPL-2.0 text](https://www.mozilla.org/en-US/MPL/2.0/)

full_moon_derive remains unmodified and comes from the registry source in
`Cargo.lock`. The dependency-policy exception is limited to the approved StyLua,
full_moon, and full_moon_derive versions.
