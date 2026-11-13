# ADR-0026: Separate the generated contract into a dedicated crate and standardize on a manifest-centered CLI

- Status: Accepted
- Implementation: Implemented
- Validation: macOS local / MSRV and stable / Reviews A and B complete. See
  validation scope and untested environments.
- Date: 2026-09-06
- Decision owners: Pythia maintainers
- Generated ABI target: 6
- Related: ADR-0027,
  implementation goal prompt

2026-09-07 amendment: [ADR-0028](0028-semantic-generated-layout-and-handwritten-line-budget.md)
removes the 350/400-line limit for generated output and replaces it with stable,
semantic partitioning. All other contracts and additional implementation
decisions remain.

2026-10-17 implemented amendment:
[ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) targets ABI 7,
removes projection/scaffold surfaces, and registers one application-error mapper
for complete resolver traits. Standalone crate ownership, manifest-driven
generation, producer identity, transactions, and semantic output remain.
ABI 6 and this ADR's recorded acceptance are historical; current execution and
regeneration use ABI 7 under ADR-0053.

## 1. Context and replacement scope

chigiri.rs generates resolver traits, projection plans, input types and schema
descriptors from GraphQL SDL. A manifest-driven workflow makes generation,
explicit updates and difference checks reproducible.

Lab01 currently generates Rust modules inside an application and receives
schema, configuration, output, and Cargo settings through long CLI argument
lists. It moves to a dedicated generated crate so the boundary between
handwritten and generated code, repeated operations, and producer identity are
clear. A crate containing only traits can compile independently; that success
does not prove that application resolvers are complete.

Backward compatibility is not retained before production. This ADR replaces the
module-output, old-CLI, configuration, generated-ABI, and consumer-wiring parts
of ADR-0002, ADR-0007, ADR-0008, ADR-0018, ADR-0020, ADR-0021, and ADR-0024. It
leaves no old mode, alias, configuration reader, or duplicate generated public
surface. Complete resolver contracts, explicit projections, GraphQL execution
semantics, and diagnostic truth and redaction contracts remain.

Historical research observations are separate from the current contract and
do not select a build revision.

## 2. Dedicated generated crate

One manifest generates one crate for one GraphQL schema. A schema may contain
multiple SDL files. Independent schemas use separate manifests; no profiles,
batch targets, or per-command overrides are introduced.

```text
consumer/
  pythia.toml
  graphql/                         # handwritten SDL
  generated/graphql-contract/      # entirely generated
    Cargo.toml
    pythia-generated.json          # closed ownership/provenance/inventory
    src/lib.rs                      # public re-exports / ABI guard / generator()
    src/__pythia/...                # private shards by responsibility
  src/graphql/...                  # handwritten Types / resolvers / projections / assembly
```

- The generated crate does not depend on the application or domain crate.
  Associated types connect objects, scalars, and domain enums.
- The public code-generation library entry point uses the same resolved manifest
  and generation graph. The CLI alone does not adopt the new model while the
  library retains old module output or a different producer-stamp rule.
- Generated `Cargo.toml` declares a standalone `[workspace]` and
  `publish = false`. Applications use an ordinary path dependency. Generation
  does not register the crate as a member of a parent workspace. A consumer that
  places it within its workspace uses dedicated output outside any member
  directory and adds it to `workspace.exclude`. MSRV Cargo still rejects a
  nested independent workspace under a member even when excluded. For workspace
  consumers, the Pythia manifest is placed so the workspace Cargo.toml and
  lockfile remain within the same manifest root.
- The generated `pythia` façade dependency comes from the runtime in Section 3.
  Code-generation and CLI crates are not included as runtime dependencies.
- Input types, closed wire enums, resolver traits, projections, and aggregation
  are public crate-root contracts. Shard paths and names are private and never
  referenced by handwritten code.
- Adding SDL exposes missing root fields/groups, projection fields, and type
  bindings through consumer compilation. No default resolver, reflection,
  catch-all enum, or successful unimplemented stub is added.
- `generate` and `update` validate the generated crate alone. Requiring an
  unchanged consumer to compile before regeneration would break the intended
  schema-change, regenerate, then implement cycle. `doctor` validates consumer
  completeness.
- Handwritten source, tests, Cargo.lock, target output, and caches do not enter
  the generated inventory. The generator never edits the consumer Cargo.toml,
  lockfile, or source.
- Handwritten `.rs` and `.sh` files remain at most 400 physical lines, with 350
  as the normal split point. Generated output follows ADR-0028's semantic
  partitioning and has no line limit. Resource limits, formatting, compilation,
  exact checks, and English rustdoc remain required. Compile fixtures cover
  namespace, keyword, re-export, and generated-macro hygiene across crates.

The marker schema is `pythia/generated-crate-v1`. Every level is closed. It
contains package name/version/edition, ABI, `GeneratorInfo`, explicit runtime,
provenance, and file inventory. Provenance records the manifest's relative label
and full-byte SHA-256, the existing `SchemaDigest`, semantic binding digest, and
each SDL file's relative path, byte count, and SHA-256. To avoid a self-referential
hash, inventory lists every file except the marker itself in path order with
path, byte count, and SHA-256. A private
`src/__pythia/generator_info.rs` duplicates generator information in Rust;
crate-root `generator()` exposes it. Rust/JSON consistency and the marker itself
are still exact-check inputs: excluding the marker from its own inventory does
not exclude it from comparison.

### 2.1 Resolve the orphan rule for domain enum bindings

When generated code moves into a separate crate,
`impl generated::FooBinding for domain_crate::Foo` is illegal because both the
trait and target type are external. Requiring wrappers around every external
domain type is also rejected.

Binding authority moves into the application-local
`Types: generated::SchemaTypes`. For each opted-in enum, generation adds a domain
associated type and required bidirectional conversion methods to `SchemaTypes`.
The old `FooBinding for Foo` form is removed. Other associated types and methods
are omitted from this example:

```rust
impl graphql_contract::SchemaTypes for Types {
    type AdapterKind = domain::AdapterKind;

    fn adapter_kind_from_graphql(
        value: graphql_contract::AdapterKind,
    ) -> Result<Self::AdapterKind, pythia::EnumBindingError> {
        match value {
            graphql_contract::AdapterKind::V3 => Ok(domain::AdapterKind::V3),
            graphql_contract::AdapterKind::V4 => Ok(domain::AdapterKind::V4),
        }
    }

    fn adapter_kind_to_graphql(
        value: &Self::AdapterKind,
    ) -> Result<graphql_contract::AdapterKind, pythia::EnumBindingError> {
        match value {
            domain::AdapterKind::V3 => Ok(graphql_contract::AdapterKind::V3),
            domain::AdapterKind::V4 => Ok(graphql_contract::AdapterKind::V4),
        }
    }
}
```

Method names use existing Rust-name normalization and collision checks.
`SchemaTypes` is an aggregation responsibility and is not line-sharded merely
because it is large; any internal structure follows ADR-0028 responsibility and
resource reasoning. ABI 6 escapes type and variant names `Self` and `_`, which
cannot be raw identifiers, as Clippy-compliant reserved names
`__PythiaNameSelf` and `__PythiaNameUnderscore`. Snake-case positions retain
`__pythia_name_`; both forms reject collisions after escaping. Runtime and
standard-library references use absolute `::pythia` and `::std` paths. Coercion
and serialization always use these conversions and preserve existing failure
masking. Domain enum discriminants are never inferred from GraphQL strings.
Consumers using only the standard generated enum implement no conversion. A
three-crate fixture—domain, generated, and application—proves legality and
missing-implementation detection.

## 3. The sole configuration input: pythia/config-v2

The default manifest is `pythia.toml`. Its schema is closed at every level and
rejects unknown or duplicate keys, old `version = 1`, and the old
`domain-enums` spelling. There is no implicit conversion from old configuration.

```toml
schema_version = "pythia/config-v2"

[schema]
directory = "graphql"

[package]
name = "graphql-contract"
version = "0.1.0"
edition = "2024"

[generation]
output = "generated/graphql-contract"

[runtime]
source = "path"
root = "../chigiri-source"

[bindings]
domain_enums = ["AdapterKind"]

[consumer]
cargo_manifest = "Cargo.toml"
package = "my-app"
generated_dependency = "graphql_contract"
features = []
all_features = false
no_default_features = false
all_targets = true
```

Required values are `schema_version`, `schema.directory`, all three package
fields, `generation.output`, and `runtime`. Omitting bindings means an empty
`domain_enums` array. Duplicate, missing, or non-enum binding targets fail.
Consumer configuration is optional for generation and required for doctor.
The optional `package.license` is the caller's Cargo license expression, a
nonempty bounded string with no control characters or whitespace-only value.
It is emitted verbatim through TOML escaping in both installed and private
candidate Cargo manifests. Chigiri neither selects a license nor inherits one
from its own package. Omission preserves the former generated Cargo bytes;
`license-file` and automatic license-file copying are not supported. Adoption
uses ordinary protected `rust update`, never a manual generated-file edit.
The existing manifest provenance hash and Cargo inventory cover the field;
config-v2, the ownership marker format and generated ABI are unchanged.
When present, `cargo_manifest`, `package`, and `generated_dependency` are
required. Defaults are empty features, `all_features = false`,
`no_default_features = false`, and `all_targets = true`. Combining
`all_features` with nonempty features or `no_default_features` is rejected.
`generated_dependency` is the dependency alias selected in the consumer's
Cargo.toml; doctor verifies the actual wiring.

Git runtime is the following closed alternative:

```toml
[runtime]
source = "git"
url = "https://example.org/team/pythia.git"
revision = "<actual complete lowercase 40-character commit>"
# Optional package version assertion at that commit.
version = "0.1.0"
```

- Git accepts ordinary HTTPS and SSH URLs supported by Cargo, while rejecting
  passwords, tokens, query strings, fragments, and control characters. Revision
  is exactly 40 nonzero lowercase hexadecimal characters. Branches, tags, short
  hashes, dirty state, and unknown revisions are not emitted as Cargo revisions.
  Example URLs and revisions are never represented as a real release. The tool
  neither requires SSH nor enforces a transport or custom authentication.
- The optional Git-only `runtime.version` is the actual runtime package SemVer,
  without a Cargo requirement operator. It emits `version = "=<SemVer>"` alongside
  `git` and `rev` in both installed and private candidate Cargo manifests. Cargo
  must verify that the selected commit provides that package version; the
  assertion never switches the source to a registry or changes the commit.
  Omitting it preserves source-only selection. Do not infer it from the generated
  package version or GeneratorInfo: generator, generated package, and runtime
  versions remain independent under ADR-0027. The existing manifest provenance
  hash and Cargo inventory record cover this assertion; marker formats and the
  generated ABI are unchanged.
- Path root is a manifest-relative chigiri.rs source root. Ordinary `.` and `..` are allowed;
  absolute paths and ambiguous separators are rejected. The fixed
  `root/crates/facade` is selected and rebased to a Cargo path relative to the
  SDK root. The runtime root is not modified. Path sources are mutable and
  location-dependent and are never labeled immutable or released.
- Schema, output, and consumer file paths use normal manifest-root-relative
  components only. Absolute paths, `.` or `..` traversal, symlinks, special
  files, and overlapping input/output are rejected. This boundary differs from
  the explicit allowance for a runtime Path dependency. The manifest belongs at
  a common ancestor of required input, output, and consumer paths.
- The manifest selected by the CLI may itself be an absolute or relative path
  containing ordinary `.` and `..`. Ancestors and the file are captured without
  following links; resolved configuration paths are never reinterpreted from
  the current directory. Provenance and location diagnostics use the basename.
  I/O diagnostics retain UTF-8, limit, and kind facts after common redaction.
  The old config-v1 reader restricted to the current directory is removed.
- Package name, version, and edition are validated as Cargo values. Edition must
  be 2024 and generated `rust-version` is 1.85. Tool-side version validation uses
  `semver`, while core retains zero normal dependencies. Package-name validation
  is distinct from Rust identifier normalization. The CLI cannot override
  package or feature values.
- No new variable resource-policy setting is added. Fixed limits are centralized
  in Section 5.

Generator and runtime are separate identities. Instead of requiring their
commits to match, mismatches are detected through the ABI guard, real crate
compilation, and doctor's Cargo metadata. The consumer and generated crate must
resolve to one Pythia package identity. Different Git URLs or mixed Path/Git
sources are not silently repaired with patches or fallbacks.

## 4. CLI operations

The `cargo-pythia` binary and `cargo pythia` Cargo subcommand remain. The only
global options are `--manifest <path>`, `--format human|json`, and
`--deny-warnings`. Root and subcommand `--help` and `--version` require no
manifest, Git, network, or Cargo. Version details follow ADR-0027.

```sh
cargo pythia --manifest pythia.toml init
# Edit the manifest and SDL, then select a runtime source explicitly.
cargo pythia generate
# Add the path dependency to consumer Cargo.toml and implement resolvers.
cargo pythia check
cargo pythia doctor
# After changing SDL:
cargo pythia update
# After correcting consumer compile errors:
cargo pythia doctor
cargo pythia snapshot --out schema.snapshot.json
cargo pythia diff previous.snapshot.json schema.snapshot.json
cargo pythia scaffold projection --object Book
```

| Command | Contract |
| --- | --- |
| init | Exclusively creates only the manifest. It generates no SDK and overwrites no existing file, directory, or link. |
| generate | Generates only when output does not exist; even an empty directory or existing generated crate is rejected. |
| update | Exchanges a candidate directory only with an existing crate carrying a valid ownership marker; it does not modify the manifest. |
| check | Uses recorded generator information for exact comparison and changes neither output nor manifest on success or failure. |
| doctor | After static manifest, schema, ABI, and generated-output checks, checks completeness through the real consumer Cargo project. |
| snapshot | Generates a versioned canonical snapshot from the manifest schema to an explicit report destination. |
| diff | Compares two snapshots and conservatively classifies GraphQL changes as breaking, dangerous, or operational. |
| scaffold projection | Writes only handwritten candidates for one object to stdout; the generated crate name comes from consumer configuration or the package name. |

`init` writes active defaults for schema, package, output, and bindings, with
runtime and consumer examples commented out. Generation is unavailable until
the user selects a runtime. It emits no fictional URL or revision, empty
successful schema, or release claim. The old
`generate --schema-dir/--config/--output-dir`, `schema snapshot/check`,
`--diagnostic-format`, doctor Cargo overrides, and
`--allow-lockfile-update` are removed. `--object` selects a scaffold target and
`--out` selects a report destination; neither overrides the manifest. Report
destinations cannot overwrite inputs, manifests, SDKs, or consumer source and
do not replace an existing report without consent.

Stable diagnostic codes, severity, origin, schema coordinates, labels, help,
notes, and rustc correlation remain. When all-targets reports the same source
diagnostic from library and test targets, original rustc records are both kept;
only Pythia supplements whose code, coordinate, label, help, and notes are
identical are deduplicated. Supplements with different locations or content are
not combined. For a chain beginning with an explicit external builder, missing
stages reported at a later `with`, `project`, or `resolve` method—not only at
`build`—can correlate to schema order. Parentheses in ordinary strings inside an
earlier callback do not delimit the expression. Unrelated statements, errors
inside callbacks, and same-named modules are excluded.

Generated-integrity failure in doctor adds `SF4016` while retaining the causal
`SF4009` (drift), `SF4020` (marker/inventory), or `SF4021` (producer record).
Drift from a valid record suggests update. A corrupt recorded tree suggests
saving existing bytes under a new name and generating to an absent destination.
I/O, cancellation, and tool failures are not mislabeled as drift. A difference
between a structurally validated marker and tree inventory, or between the tree
and rerendered expectation from a valid record, retains missing/extra/changed
counts and SDK-relative paths. Common redaction and text/supplement limits apply
to path lists; if paths are omitted while counts remain, `truncated` and
`SF4007` are explicit. Advice for a non-default manifest preserves its shell-
quoted name and uses the manifest directory as the execution origin. No path or
input value is copied from an invalid marker before validation.

`--format json` produces one bounded JSON document; there is no old JSONL
reader. A report is
`{schema_version: "pythia/diagnostics-v2", diagnostics: [...], truncated: bool}`.
Each diagnostic has `origin` (`pythia`, `rustc`, or `cargo`), `code`, `severity`,
`message`, nullable `coordinate`, `labels`, `help`, and `notes`. A label has
primary status, relative path, byte start/end, line/column, and message. Raw
source excerpts are absent from the model; human output renders the same
redacted record.

Each text is limited to 16 KiB at a UTF-8 boundary, the report to 1,000
diagnostics and 1 MiB, and each diagnostic to 32 labels, help items, and notes.
Limit truncation sets `truncated = true` and adds an `SF4007` error. A common
ingress for the owned model handles absolute paths, URIs, credential keys and
values, and terminal control sequences. `Debug` and `Display` retain or reveal no
unprocessed source or secret. Scaffold keeps diagnostic JSON on stderr and Rust
candidates on stdout. Other commands use stderr for diagnostics and stdout or an
explicit file for version, data, and reports. Common redaction applies to
messages, labels, help, notes, and malformed Cargo output.

Exit 0 is success; exit 1 is an input, generation, compatibility, or consumer
check failure; exit 2 is an invalid argument, execution-environment failure, or
tool failure. Library generation and update accept a resolved manifest and
`DiagnosticPolicy`, returning warnings and notes even on success.
`deny_warnings` applies after candidate compilation and before commit. An update
rejected by warning policy retains the old tree and reports `SF4028`. A
difference between recorded and executing generator information is an `SF4027`
note. Unknown options exit 2 instead of warning and being ignored.
Informational notices are not denied as warnings. Diff exits successfully for no
change or non-breaking change; Breaking, Dangerous, or Operational change
requires review and exits nonzero while reporting the classification.

## 5. File/process boundary and directory updates

Removing a distribution-environment isolation assumption does not remove input
and output protection. Linux and macOS Unix hosts are the supported CLI
filesystem targets for this change; a host without required capabilities fails
before any write. Root help/version and pure in-memory snapshot comparison are
not restricted. Snapshot file I/O through the CLI uses this boundary. GraphQL
runtime and generated-crate target portability remain unchanged.

Fixed limits retain existing discovery and parser limits: at most 64 KiB for the
manifest, 256 KiB per SDL, 16 MiB total SDL, 1,024 sources, discovery depth 64,
and 65,536 entries. Output is at most 64 MiB, 65,536 files, depth 64, and a
1 MiB marker. Limits apply during rendering and actual-tree traversal. Any
smaller existing limit at implementation start remains; changing it requires an
ADR rationale.

- Manifest, input, and output components use no-follow operations relative to
  retained handles. Kind checks and bounded reads use the same descriptor and
  detect symlinks, FIFOs, devices, path swaps, and growth.
- The structured marker contains current schema, package ownership, complete
  generated-file inventory, and provenance. Before update, validate the old
  tree's ownership and resource limits; reject unknown files, missing markers,
  and fake old modules. An intentional manifest update is allowed, so equality
  of old and new manifest digests is not an update precondition.
- Concurrent generate/update of one output is exclusive. Check compares one
  captured consistent tree. A detected symlink replacement or race does not
  redirect work to another directory.
- Build a candidate in a sibling directory on the same filesystem, then verify
  formatting, semantic layout, resource limits, compilation, and ownership.
  Private compilation uses exact source bytes; only internal rebasing of Cargo
  paths may modify the candidate.
- Update renames old to backup and candidate to live. Any pre-commit failure
  restores old. If restoration fails, old is not deleted; report the primary
  cause, rollback failure, and recovery procedure together.
- The commit point is explicit. Failure to clean a backup after it does not mean
  the old tree was restored. Before deletion starts, retain a complete recoverable
  copy or archive of old bytes, so partial deletion remains
  recoverable. Candidate, backup, and recovery operations target only their named
  directories.
- Two renames are not described as an atomic exchange or zero-downtime update.
  The transaction handles catchable process failures but does not promise fully
  automatic recovery after `SIGKILL` or power loss. A later invocation reports
  ambiguous candidate or backup remnants with recovery instructions instead of
  ignoring them.

Transaction placement is fixed. A non-blocking `flock` on the output parent's
inode serializes the same output, including case and Unicode-equivalent forms;
different outputs under one parent also serialize. The lock descriptor is
opened independently so calls in the same process contend. `Drop` unlocks
explicitly, preventing a descriptor briefly inherited by parallel spawn from
delaying release. The OS releases it on process exit. A removable lock file is
not the authority.

- Exclusively create `.pythia-transaction` in the parent. Store the target name
  in `state.json`, the new tree in `candidate`, renamed old tree in `previous`,
  and validation crate in `compile`. Never reuse or delete remnants silently.
- The commit point follows candidate-to-live rename and revalidation of live
  bytes, input provenance, and retained handles. A sync failure after rename
  passes the destination descriptor to rollback.
- Sync old file bytes into `.pythia-recovery` before deletion. The archive uses
  `pythia/recovery-v1` magic; a big-endian `u32` output-name byte length and
  UTF-8 name; a `u64` file count; and, in path order, each file's big-endian
  `u32` path-byte length, `u64` content-byte length, UTF-8 path, and raw bytes.
  The archive has a fixed 68 MiB limit for 64 MiB of generated bytes plus marker
  paths and indexes. Empty directories are forbidden by the ownership marker and
  can be reconstructed from paths.
- After backup and transaction cleanup completes, remove the archive with one
  final unlink. No fallible cleanup follows that successful unlink. A cleanup
  error states that the new live tree is already installed. The
  [recovery script and procedure](../generated-crate-recovery.md) restore into an
  explicitly absent directory.
- A change to manifest bytes or SDL provenance during processing fails before
  commit. Existing input/output/cache overlap uses retained ancestor inodes.
  An absent suffix whose case or Unicode equivalence cannot be decided is
  conservatively rejected as overlapping.

The existing pinned rustfmt 1.85.0 plus embedded configuration remains. Required
tools are diagnosed during preflight; the CLI never runs `rustup install`.
Formatter and Cargo I/O are bounded, with a 300-second process-tree deadline and
kill/reap on cancellation. Blocking receive without a timer is prohibited. Safe
`rustix` APIs handle retained descriptors, non-blocking pipes, and process groups;
no `unsafe` enters project source.

The CLI converts SIGTERM and SIGINT into a cancellation token through the safe
`signal-hook` flag adapter. Synchronous library operations inherit an explicit
`Cancellation::run` thread scope; propagation to other threads and signal
registration are never implicit. Cancellation fails with `SF4026` and exit 2
after launched tools are reaped. Cancellation is deferred during rollback and
recovery cleanup so repeated signals cannot interrupt restoration of old bytes.
Post-commit cancellation reports that the new live tree is installed. The signal
adapter is for a CLI that owns the disposition until process exit; unregistering
a handler is not treated as restoring the previous OS disposition. Build-
information collection uses a process boundary independent of this CLI signal
scope.

Doctor uses the same process boundary; the old `command-group` dependency and
dedicated process implementation are removed. Cargo uses normal configuration
and caches, while private source, lockfile, and target locations must not overlap
the SDK. Validate target/build locations returned by
`cargo metadata --no-deps` for the private crate. Preserve explicit settings;
only the default private `target` moves to the output parent's
`.pythia-cache/target` for reuse. Private Cargo.toml changes only rebased Path
dependencies, and Rust bytes equal the candidate before and after validation.
Private compilation creates its own required lockfile. Consumer doctor uses
`--locked`; when an update is required, it directs users to normal Cargo
operations. No arbitrary shell-command setting is added. Doctor confirms a real
dependency on the current generated crate and relates rustc diagnostics to SDL
and handwritten implementation. It does not claim to sandbox all side effects
of trusted build scripts.

### Doctor Cargo adapter and diagnostic correlation

After static exact check, `doctor_contract` requires consumer configuration. It
uses the requested Cargo.toml parent as Cargo's working directory and inherits
normal Cargo configuration, cache, and environment. It uniquely selects the
consumer package from `metadata --locked --no-deps` workspace members. Full
metadata and check then name that package's actual Cargo.toml. Pythia does not
expand virtual-workspace glob/exclude rules and passes feature expressions,
including `dep/feature`, unchanged to Cargo. A change in selected package or
actual workspace location between the two metadata calls is an error.

- The requested, selected, and workspace Cargo.toml files and workspace
  Cargo.lock remain inside the manifest root, are at most 1 MiB each, and are
  read without following links. Manifest, SDL, and SDK are revalidated before
  and after tools run. No lockfile is created or updated implicitly. A known
  lock-update requirement or unknown feature is an input failure with exit 1,
  without copying raw stderr. Other metadata tool failures exit 2.
- Metadata validates required version-1 fields and graph references while
  allowing unused fields added by Cargo. Package, member, node, and edge counts
  are each limited to 65,536; package ID and source to 16 KiB; process output to
  64 MiB. The normal dependency closure begins at the selected consumer, plus
  that consumer's development dependencies only when `all_targets=true`. Build
  dependencies do not establish runtime identity. Target-specific dependency
  graphs are checked conservatively.
- The configured dependency alias must match the generated crate's name,
  version, Path source, and directory inode and must have an allowed dependency
  kind. A normal dependency is accepted, as is the selected consumer's
  development dependency when `all_targets=true`; build-only aliases are not.
  This supports test-only consumers such as testkit. Follow the generated crate's
  normal `pythia` dependency to runtime, rejecting multiple identities with the
  same name among Pythia runtime, adapters, and testkit in that closure. Path
  crates are checked by inode against fixed repository locations. Git crates
  must all use the façade's source, and the exact-checked SDK's Cargo-normalized
  `dependencies[].source` request must match the resolved source. The adapter
  accepts only `git+<Cargo URL>?rev=<complete revision>#<same revision>`. It does
  not normalize host case or default ports, and rejects resolution through a Git
  patch to another source. Because the
  [Cargo metadata specification](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)
  treats ID/source representation as opaque, unknown Git forms are not guessed.
  A representation change is a tool failure and requires the adapter and real
  Cargo fixture to change together.
- Reject target/build caches overlapping the SDK, SDL, or target source
  directory of the selected package before check. A normal adjacent consumer
  `target` is allowed. This is not a sandbox for arbitrary build-script effects.
- Resolve relative rustc spans from the actual workspace root and create relative
  labels only inside the manifest root. Supplemental source uses no-follow reads
  from the same descriptor, limited to 1 MiB per file, 16 MiB total, and 1,024
  cache entries. If source is unreadable or over limit, omit SDL guidance while
  preserving rustc facts. Raw excerpts never enter the report.
- SDL guidance applies only to rustc diagnostics for the selected package. A
  missing root trait matches either a generated span or an implementation of the
  explicit external trait. A missing stage requires agreement among a continuous
  external-alias builder chain in real source, rustc stage number, and schema
  order. An output mismatch requires the primary span to match a simple callback
  result in that builder. Pythia does not infer from unrelated statements or
  errors inside callbacks. A complex expression with insufficient evidence
  receives only the original rustc diagnostic. Source/JSON correlation starts
  from package ID, code, and span in
  [Cargo JSON messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages),
  never by reparsing rendered text.

Generate complete resolvers and domain associated types even for SDL objects not
referenced by another field or composite. Do not generate unused private
conversion helpers. The required set follows all source-object field outputs and
arguments, reachable input fields, and possible objects of referenced interfaces
and unions. Public types, associated types, and complete resolvers remain even
for unreferenced types. A projection shard containing only implementations does
not glob-import its parent or introduce unused imports into the standalone crate.

## 6. ABI, digest, snapshot, and migration

The semantic binding digest uses `pythia/domain-enums-v1` followed by a NUL byte
as domain separator. In lexicographic enum-name order, SHA-256 receives each name
as a big-endian `u64` byte length followed by its UTF-8 bytes. Only when bindings
are nonempty are those 32 bytes added through FNV update to the existing resolver
schema fingerprint and the component count increased by one. Manifest comments,
runtime, package, output, and `GeneratorInfo` do not enter this type-ABI
fingerprint; full manifest provenance and full-byte checks detect them separately.

- Generated/runtime supports only ABI 6. A compile guard rejects mixing ABI 5
  or earlier, with no compatibility layer that keeps reading old modules. An ABI
  number change alone is not a migration.
- The existing `SchemaDigest` algorithm and value remain for identical relative
  source paths and SDL bytes. CLI build information, runtime source, and
  package/output paths do not enter it.
- Resolver fingerprint canonicalizes ABI 6 binding semantics. It does not hash
  the full manifest, including comments, order, or destination, as type-binding
  identity. Full manifest, input, and provenance drift is checked separately.
- Snapshot becomes current-only `pythia/schema-snapshot-v2`; old formats are
  rejected explicitly. Generator/runtime/provenance differences are separate
  from GraphQL semantic and implementation-surface differences, so metadata
  alone is not reported as wire breaking.
- Books, Axum adoption, the large consumer, testkit, golden and compile-fail
  fixtures, and all scripts, CI, and examples migrate to the canonical CLI.
  Generated files are not hand-edited during migration. Handwritten consumer
  resolvers remain outside the generated crate.
- This decision owns repo-local fixtures and representative domain bindings.
  Application migrations require their own procedure.

### snapshot-v2 and supporting commands

Snapshot is produced from compiled current manifest/SDL and requires no SDK,
Cargo, or formatter. GraphQL semantic items preserve existing signature,
default, description, deprecation, and operational metadata and are recorded
uniquely by item kind and coordinate. Schema digest remains the source digest.
Additional data includes ABI 6; domain enum names and semantic binding digest;
the resolver fingerprint from the current renderer; `GeneratorInfo`; Git or Path
runtime; package and output; manifest label/hash; and each SDL file's relative
path, byte count, and hash. The record is neither a signature nor proof of source
authenticity. It contains no raw SDL or absolute host path.

The reader is closed at every level and rejects missing nullable fields,
duplicate fields/items/sources, unknown kinds/versions/ABI, invalid digests,
`GeneratorInfo`, runtime or binding data, and resource-limit violations. It
retains no old snapshot reader or old directory-only API. Item coordinates and
signatures plus parent and referenced kinds are validated before diffing. Public
in-memory parse, serialize, and diff operations remain available on hosts without
filesystem support. Snapshot input/output retains the 32 MiB limit, with at most
1,048,576 items, 256 KiB per text, and type-reference nesting depth 64. The item
count must not needlessly reject a small-item sequence within 32 MiB; bytes remain
the effective boundary. Source inventory uses the SDL limits of 1,024 files,
256 KiB each, and 16 MiB total. Writer JSON has fixed field, item, and source
order plus a trailing newline. The reader permits whitespace variation but does
not discard meaningful records silently.

Every diff change has a `graphql`, `implementation`, or `metadata` domain.
Existing GraphQL classifications remain; additional records are:

| Difference | Domain | Classification |
| --- | --- | --- |
| domain enum binding | implementation | Breaking (changes the `Types` contract) |
| resolver fingerprint | implementation | Operational (recheck generated-adapter wiring) |
| runtime source, package/output | metadata | Operational |
| generator, manifest/source provenance, schema source digest | metadata | Non-breaking |

A GraphQL-equivalent reorder that changes the existing order-dependent resolver
fingerprint is recorded only in the implementation domain, never as GraphQL wire
breaking. Default policy exits 1 for Breaking, Dangerous, or Operational changes
in any domain. Diff compares only its two snapshots and reads no current
manifest, SDL, or SDK; global `--manifest` has no target for diff. Snapshot paths
and the `--out` report destination are relative to the CLI working directory,
distinct from manifest-relative settings. Diff data is human output or
`pythia/schema-diff-v2` JSON on stdout; diagnostics use one diagnostics-v2
document on stderr. Report output is exclusively created outside manifest, SDL,
SDK, and consumer source. Existing reports are not replaced, and the parent must
already exist; snapshot does not create parent directories.

The scaffold library entry accepts only a resolved manifest and object name. Its
external crate path prefers the consumer dependency alias and otherwise converts
package-name hyphens to underscores. It raw-identifier-escapes Rust keywords and
uses leading `::` for the external crate. Root, wrong-kind, or unknown objects
exit 1; old module/config/schema/write overrides exit 2. Only candidate Rust is
written to stdout. When forming private projection types from escaped object
names such as `Self`, the name encodes the count of leading underscores and
retains CamelCase, preventing distinct SDL names from collapsing. Prefixed
resolver/projection methods and private conversion helpers likewise encode the
leading-underscore count numerically instead of generating warning-prone internal
double underscores such as `with__type`.

## 7. Acceptance

The normative gates are in
[spec-conformance Section 15](../spec-conformance.md#15-sibling-generator-conformance).
CLI/manifest/lifecycle, three-crate orphan/completeness, existing GraphQL
contracts, full and large consumers, cross-crate macro/naming, bounded process
and I/O, and generator records each have independent positive and negative
tests. ADR-0027 product validation decides implementation completion without
requiring successful publishing, packaging, or SSH authentication.

## 8. Consequences

Generated and handwritten code are separated at a crate boundary, and the
schema-change-to-regeneration workflow becomes concise and reproducible. Initial
setup must explicitly configure a path dependency and runtime identity and
migrate enum bindings. Crate separation alone does not detect missing resolvers
earlier; separating generated-crate validation from consumer doctor permits
regeneration while still presenting implementation gaps promptly. The design
keeps generation operations independent from application backends and deployment
policy.


## 2026-11-09 publication-fixture amendment

The former captured application corpus is retired from public source and test
inputs. Its replacement is the repo-owned [full-schema fixture](../../tests/fixtures/full-schema/README.md):
11 files, at least 30,735 input bytes, 67 objects/709 fields, 19 inputs, 65 bound
enums and 5 scalars. Independent domain enums retain their three/two/three
variant vectors, real generated-argument conversions and complete object resolver
coverage. SG-VERIFY-02 and SG-LAYOUT-07/08 keep their IDs; their source and digest
authority is the current fixture inventory, canonical generator and Rust consumer
tests. No backend-specific paths or live application checkout are required.

Generate the new SDK through the normal CLI and validate complete ownership and
isolated byte-for-byte reproduction. Old producer observations and qualification
results stay historical; they cannot certify these replacement inputs.
