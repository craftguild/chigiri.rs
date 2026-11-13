<h1 align="center">chigiri.rs</h1>

<p align="center">
  <a href="https://github.com/Essk/ai-contribution-level">
    <img src="https://raw.githubusercontent.com/Essk/ai-contribution-level/main/badges/level-3.svg" alt="AI Contribution">
  </a>
</p>

chigiri.rs is a schema-first GraphQL toolkit for Rust backends and frontend teams.
One schema generates typed Rust resolver contracts and editable Luau mock servers.
The `chigiri` command contains both workflows. Version 0.1.0 is the initial
release. Use the CLI binaries below or build the CLI and libraries from source.

## Quick start

Release binaries are available for Linux x86_64 GNU and macOS arm64 from
[GitHub Releases](https://github.com/craftguild/chigiri.rs/releases/tag/v0.1.0).
The installer needs POSIX sh, curl, and `sha256sum` or `shasum`. It verifies the
downloaded checksum and installs to `~/.local/bin` by default.

Install the CLI, then create a local mock project:

```sh
curl -fsSL https://raw.githubusercontent.com/craftguild/chigiri.rs/v0.1.0/scripts/install.sh | sh -s -- --version 0.1.0
BIN="$HOME/.local/bin/chigiri"
mkdir -p target/hello
printf 'type Query { hello: String! }\n' > target/hello/schema.graphql
"$BIN" mock init --schema target/hello/schema.graphql --config-dir target/hello/mock
"$BIN" mock check --config-dir target/hello/mock
```

The check reports `readiness_checked = true`. The initial resolver deliberately
returns `NOT_IMPLEMENTED`; edit its Luau body to implement the query. Continue
with the [Mock quick start](docs/guides/mock-quick-start.md) to serve a Todo API.

Pass `--bindir /absolute/bin` after `--version 0.1.0` to choose another directory.
Use `--uninstall` instead of `--version 0.1.0` to remove the command; pass the same
`--bindir` for a custom destination. Add that directory to PATH for bare commands.
Each release includes target-specific `SHA256SUMS` files and a CLI SBOM.

## System requirements

The CLI runs on macOS Apple Silicon and Linux x86_64. Intel macOS and Windows
MSVC are build-only Tier 2 targets, described in the [platform guide](docs/building.md).
Mock execution and offline help/version/SBOM need no Node.js, Cargo, Git or
external Luau interpreter; GraphiQL assets are embedded in the executable.

Rust application libraries and generated contracts require Rust 1.95 or later.
Generation uses Rust 1.95.0's formatter. `rust doctor` also needs the application's
Cargo dependencies. Applications own their HTTP listeners, routing and shutdown.

## Build requirements

Use Rust/Cargo **1.95.0**, rustfmt, a native linker, GNU Make, a POSIX shell,
Git, curl, Python **3.11+** and Node.js/npm. CI selects Node.js **24.15.0**.
The first-party package minimum is Rust **1.95**.

`make setup` explicitly restores standards **v0.6.1** from the fixed reference.
The published engine hosts are macOS arm64 and Linux x86_64 GNU. Ordinary
operations use the verified cache; no global standards installation is required.
Prepare Cargo and GraphiQL's locked dependencies before building. GraphiQL's
ordinary regeneration uses its prepared npm cache offline; its audit reads
current advisory metadata.

Quality checks additionally use Clippy, tokei **15.0.0**, cargo-deny **0.19.8**,
ripgrep (`rg`), `jq` and the private development dependencies prepared with `npm ci --ignore-scripts
--no-audit --no-fund` at the repository root. Ordinary checks use repo-owned
metadata fixtures, create no Git commits or signing keys, and require no GPG.

```sh
make check
make acceptance PROFILE=debug
make acceptance PROFILE=release
```

Make and direct pinned launcher test/check run the same source-bound product
catalog, executing it when no valid current result exists. Do not overlap Cargo
commands in one build directory. [Validation](docs/validation.md) separates ordinary
checks from full/weekly robustness campaigns.

Release and debug output are `target/<target-triple>/<profile>/chigiri`.
`PROFILE`, `TARGET` and `CARGO_TARGET_DIR` select matching build/install artifacts.
`make install BINDIR=/absolute/bin` copies an already verified executable.
[Build guidance](docs/building.md) covers cross-target SDKs and deliberate release
preparation. `make release` changes versions/notes locally; it is not a build alias.

Build the CLI from source after preparing the build tools:

```sh
make setup
cargo +1.95.0 fetch --locked
bash scripts/python-with-toml.sh scripts/graphiql_build.py --prepare-dependencies
make
```

## Usage

For Rust backend integration, configure `chigiri.toml`, generate its dedicated
owned contract crate, and check the application:

```sh
chigiri rust init
chigiri rust codegen
chigiri rust check
chigiri rust doctor
chigiri rust --version --format json
chigiri sbom > chigiri.cdx.json
```

### Rust framework

Generated object traits follow the `ObjectResolver<C, T, E>` pattern, with concrete
names such as `QueryResolver` and `BookResolver`. Implement their methods with
your domain values and application error type, then register the groups:

```rust
let resolvers = generated::Resolvers::builder(map_app_error)
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_book(BookFields)
    .build();

let endpoint = chigiri::Endpoint::builder(resolvers)
    .context_factory(prepare_context)
    .build()?;
```

The resolver implementations, error mapper and context factory belong to the
application. The [Rust integration guide](docs/rust-integration.md),
[books example](examples/books/README.md), [subscription recipes](docs/subscriptions-walkthrough.md)
and [documentation index](docs/README.md) describe those contracts. Generated
crates use ABI 8 and config-v2; regenerate complete owned output instead of
editing generated files.

## Known limitations

- Mock is experimental and restarts from its configured seed state.
- GraphiQL and schema introspection require explicit opt-in.
- A build-only target or native Linux check does not qualify another host or
  the distribution's glibc 2.17 compatibility.
- Internal Path dependencies need explicit registry version requirements before
  crate publication. Release preparation does not publish packages or binaries.

See [support policy](docs/support-policy.md) and [conformance](docs/spec-conformance.md)
for compatibility and resource boundaries.

## Community

Read [AGENTS.md](AGENTS.md) and the [documentation reading order](docs/README.md)
before contributing. Beads owns task and review evidence under the
[review workflow](docs/review-workflow.md); tests do not read review status.

## Security

Report suspected vulnerabilities privately through [SECURITY.md](SECURITY.md).

## License

First-party code is licensed under either the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.
[NOTICE.md](NOTICE.md) identifies separate third-party terms for embedded
GraphiQL assets, the banner and retained formatter sources.
