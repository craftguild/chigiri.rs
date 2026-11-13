# ADR-0021: Identify corrective actions with structured diagnostics and a Cargo-aware doctor

> 2026-09-06 amendment: [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md)
> and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> define the next target for generated crates, the CLI, configuration, ABI, and
> distribution boundaries (Accepted / Planned). They replace only the relevant
> parts of this decision; existing GraphQL and security contracts and historical
> implementation evidence remain valid.

- Status: Accepted
- Implementation: Implemented
- Date: 2026-08-30
- Decision owners: Pythia maintainers
- Depends on: ADR-0002, ADR-0018, ADR-0020
- Generated ABI target: 5 (unchanged)

> 2026-10-17 implemented amendment:
> [ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) retires
> scaffold/projection-specific guidance at ABI 7 cutover. Doctor and structured
> diagnostics remain; guidance targets complete trait implementations and the
> common error mapper. Retired scaffold-only codes are not recycled. Existing
> evidence below remains evidence for its original implementation.

## 1. Context

Pythia diagnostics have `SF1xxx` through `SF6xxx` codes and source spans, but
`pythia_codegen::CodegenError` ultimately collapses a diagnostic into one
string. The Rust compiler safely rejects missing resolver groups and projection
typestate violations, but its long diagnostics expose private generated types
and make it difficult to determine whether the next correction is a `with_*`,
`project_*`, or `resolve_*` method.

Pythia must not reimplement Rust type checking. It should validate its own
static assets first, then run the real `cargo check`, and add guidance only to
diagnostics that it can correlate with high confidence.

## 2. Decision

### 2.1 Structured diagnostic model

The code-generation library and CLI share a public owned model:

```rust,ignore
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

pub struct CodegenDiagnostic {
    // Getters expose code, severity, message, primary/secondary labels, help, and notes.
}

pub struct DiagnosticReport {
    // Stores diagnostics in source/phase order and a truncated flag.
}

pub struct CodegenError {
    // Owns a DiagnosticReport containing at least one Error.
}
```

- `CodegenError::report()` and `into_report()` are public; `Display` and `Error`
  retain the human renderer.
- Diagnostic codes use the existing `SF[1-6][0-9]{3}` form. New free-form
  errors without codes are prohibited.
- Primary and secondary labels contain normalized relative paths, byte spans,
  one-based line and column values, and short labels. The model does not copy
  complete source text.
- Messages, labels, help, and notes are separate; correction steps are not
  concatenated into the error message.
- Compiler phase order and source order within each phase are stable. Reaching
  the maximum diagnostic count sets `truncated = true`.
- Structured output excludes application secrets, raw variables and headers,
  provider source, and absolute workspace paths.

Migrating an existing diagnostic does not change the meaning of its stable code.
New codes introduced here have these fixed meanings:

| Code | Meaning |
| --- | --- |
| `SF4014` | Invalid scaffold target or module path |
| `SF4015` | Doctor cannot uniquely resolve the Cargo manifest or package |
| `SF4016` | Doctor detects generated-output drift |
| `SF4017` | Cargo launch, lockfile, JSON stream, or toolchain tool failure |
| `SF4018` | Correlated guidance for a missing resolver group or projection implementation |
| `SF4019` | Correlated guidance for a resolver or projection output-type mismatch |
| `SF4020` | Scaffold placeholder requiring user implementation at compile time |

### 2.2 Human and JSON rendering

Every `cargo pythia` command accepts
`--diagnostic-format human|json`; the default is `human`.

- Diagnostics are written only to stderr. In JSON mode, stderr contains only
  JSON Lines records, without human prefixes or progress text.
- Command data such as scaffolded Rust or schema diff reports remains on stdout;
  selecting a diagnostic format does not change its wire shape.
- Commands that need a success summary write it to stdout. Diagnostic context,
  such as the exact doctor argv, appears as a note record on stderr.
- Human output orders `error[SFxxxx]`, location, excerpt and caret, secondary
  labels, `help:`, then `note:`.
- JSON output is UTF-8 JSON Lines with one record per line. Its closed schema has
  `formatVersion: 1`, `origin: pythia|rustc`, code, severity, message, labels,
  help, and notes.
- JSON contains no ANSI escapes, complete source text, backtraces, or absolute
  workspace paths.
- Text and label paths pass through the same redactor. It redacts Unix paths,
  Windows drive and UNC paths, `file:///C:/...`, and
  `file://authority/share/...` across whitespace, newline, quote, key-prefix,
  and punctuation boundaries. File URIs are scanned as complete URIs, including
  authority and drive prefix, rather than as ordinary paths.
- A rustc record preserves Cargo JSON semantics and is never replaced with a
  Pythia code. Any Pythia guidance is a separate immediately following record.
- Malformed or unrecognized Cargo output is retained with a `cargo:` prefix in
  human mode and as a bounded tool note in JSON mode.

### 2.3 Doctor command

The following is a supported developer command:

```text
cargo pythia doctor \
  --schema-dir graphql \
  --output-dir src/graphql/generated \
  [--config pythia.toml] \
  [--manifest-path Cargo.toml] \
  [--package package-name] \
  [--features a,b | --all-features | --no-default-features] \
  [--all-targets] \
  [--allow-lockfile-update] \
  [--diagnostic-format human|json]
```

Execution order is fixed:

1. Discover schema, parse configuration, compose, and validate.
2. Generate into memory or a temporary directory and compare file lists,
   contents, and provenance with `--output-dir`.
3. Resolve the Cargo manifest and target package.
4. If static stages succeed, always run
   `cargo check --message-format=json` for the target package.
5. Emit rustc diagnostics unchanged and add Pythia guidance immediately after
   only when correlation is highly reliable.

If a static stage fails, Cargo is not run, avoiding misleading Rust diagnostics
from stale generated source. This is not an option to omit Cargo validation:
there is no flag that disables Cargo after static success.

### 2.4 Cargo discovery and invocation

- Without `--manifest-path`, search upward from the current directory and use
  the first `Cargo.toml`.
- A package manifest selects that package. A virtual workspace or multiple
  candidates requires `--package`; no package is inferred.
- After manifest canonicalization, `SF4015` rejects symlinks outside the
  originating workspace, non-files, and non-UTF-8 paths.
- The default command is
  `cargo check --locked --package <name> --message-format=json`, plus only the
  selected feature and target flags.
- Only `--allow-lockfile-update` removes `--locked`. Arbitrary Cargo argument
  passthrough, alternate subcommands, and shell commands are prohibited.
- The caller's toolchain, target directory, and Cargo configuration are used,
  while the diagnostic prelude reports exact argv, manifest, and package.
- Doctor can fetch dependencies and run build scripts like ordinary Cargo.
  Documentation warns against running it in an untrusted repository.

Cargo stdout and stderr are drained concurrently. Hard limits are 1 MiB per
record, 64 MiB total, and 1,000 diagnostics. Exceeding a limit terminates and
waits for the child, returns `SF4017`, and leaves no undrained pipe. Doctor does
not modify application or generated source, manifests, or lockfiles, although
ordinary Cargo caches under `target/` may be created. A lockfile change under
`--allow-lockfile-update` is the sole explicitly approved side effect.

Cargo is spawned in a dedicated process group through the safe `command-group`
API. Normal exit, a stream limit, timeout, or dropping the doctor future first
requests termination of the complete group and then waits for the child,
leaving no descendants or zombies. The CLI does not implement platform-specific
process-group FFI or `unsafe`. `command-group` is a direct normal dependency of
`pythia_codegen` and is fully listed in the architecture.

### 2.5 Rust diagnostic correlation

Pythia guidance is emitted only when rustc code, primary-span ownership,
generated public symbol, and the schema-derived expected next method all match.
A substring in one error message is insufficient.

At minimum, the following cases are correlated:

- A missing `Resolvers` typestate group: `SF4018` identifies the next
  `with_<object>` and GraphQL object coordinate.
- A missing or out-of-order projection field: `SF4018` identifies the currently
  required `project_*` or `resolve_*`.
- A missing root trait method: `SF4018` identifies the GraphQL root field,
  generated trait, and method to implement.
- An incorrect resolver/projection closure output: `SF4019` identifies the SDL
  output type and expected Rust type.
- A generated ABI mismatch: the existing code identifies the exact
  `cargo pythia generate` command.

When confidence is insufficient, only the original rustc diagnostic is emitted.
Pythia never downgrades a rustc error to a warning, depends on a private compiler
API, or independently reclassifies an unsuccessful compilation as successful.

### 2.6 Exit status

| Exit | Meaning |
| ---: | --- |
| 0 | Static checks, generated drift, and Cargo check all succeed |
| 1 | A project contract fails, such as drift or a consumer compile error |
| 2 | A tool failure occurs in CLI arguments, I/O, manifest resolution, Cargo launch, or streams |

## 3. Acceptance matrix

| ID | Expected |
| --- | --- |
| DIAG-01 | A library caller can retrieve one or more structured diagnostics and the truncated state. |
| DIAG-02 | Human output presents stable code, span, label, help, and notes in deterministic order. |
| DIAG-03 | JSON Lines format 1 uses a closed stderr-only shape and contains no human text, ANSI, complete source, or absolute workspace path. |
| DIAG-04 | Doctor always runs Cargo check after schema, config, and drift checks succeed. |
| DIAG-05 | On static failure, doctor does not run Cargo and returns only the originating Pythia diagnostics. |
| DIAG-06 | Doctor automatically selects a package manifest and rejects ambiguity in a virtual workspace with `SF4015`. |
| DIAG-07 | The default `--locked`, explicit feature/target, and allow-lockfile-update argv follow the contract. |
| DIAG-08 | `SF4018` supplements missing groups, fields, and roots with the exact coordinate and next method. |
| DIAG-09 | `SF4019` supplements an incorrect output with its SDL and expected Rust types. |
| DIAG-10 | An unrecognized rustc diagnostic remains unchanged and receives no incorrect Pythia help. |
| DIAG-11 | ABI mismatch and drift diagnostics identify the exact regeneration command. |
| DIAG-12 | Exceeding a Cargo stream limit terminates and waits for the child and returns `SF4017`. |
| DIAG-13 | Exit 0, 1, and 2 distinguish success, contract failure, and tool failure. |
| DIAG-14 | An ordinary doctor run does not change application source, generated source, manifests, or lockfiles. |
| DIAG-15 | MSRV and stable classify the same fixtures without snapshots of complete rustc wording. |

## 4. Consequences

### Positive

- Compiler-enforced completeness remains while users learn the next schema
  coordinate to implement.
- Editors and CI can consume stable JSON and exit statuses.
- One command verifies schema, generated drift, and Rust compilation.

### Negative

- Correlation between Cargo JSON and generated symbols requires maintenance.
- Doctor has the duration, side effects, and build-script trust boundary of an
  ordinary `cargo check`.
- Safe process-group creation, termination, and waiting add `command-group` to
  the direct maintenance surface.
- Low-confidence cases still receive only the original rustc diagnostic.

## 5. Rejected alternatives

- Reimplement Rust type checking in Pythia: behavior would diverge from rustc
  and weaken completeness.
- Snapshot complete rustc messages: toolchain patches, locales, and wording
  changes make them unstable.
- Make doctor static-only: missing resolver implementations remain invisible.
- Make Cargo optional by default: the completion condition of one command would
  vary by user.
- Replace the original rustc diagnostic with Pythia prose: this discards the
  root cause and compiler information.
