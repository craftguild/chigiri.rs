# ADR-0035: Use clap for the CLI, ratatui for the TUI, and explicitly embed GraphiQL

- Status: Accepted
- Implementation: Implemented (all 30 DX35 IDs and Gates DX35-A through DX35-E passed locally; the user approved three additional licenses, and the UI procedure was handed off pending manual confirmation)
- Designer review: Received 2026-09-09. The integrated ADR-0034 review passed with no findings at any severity (integrated verification record).
- Date: 2026-09-09
- Decision owners: Pythia maintainers
- Depends on: ADR-0004/0016 single endpoint and bridges, ADR-0026/0027 CLI contracts, and ADR-0031 through ADR-0034 Mock operations, approval, and formatting
- Amends: ADR-0004/0008 to permit only the specific route helpers defined here, ADR-0016 to add explicit GraphiQL and introspection publication settings, and ADR-0031 to replace the external pager
- Scope: Lab01 Rust CLIs, the Mock TUI and reference viewer, fixed GraphiQL assets, neutral HTTP responses and bridge helpers, and Mock `serve`
- Activation: Follow-on work after ADR-0034. Do not add DX35 acceptance retroactively to completed work.
- Implementation goal: ADR-0034/0035 Integration Goal Prompt

Amendment dated 2026-09-09: Browser verification is limited to the minimum smoke
test in Section 7.3 and may be handed off to the user. Dedicated browser
automation and CI are not required. Report Rust and HTTP verification separately
from manual UI confirmation.

## 1. Background and decisions

The current `cargo-pythia` and `pythia-mock` commands use handwritten argument parsing and help, while the inference TUI draws its screens manually.
Reference output is sent to an external pager. Move these everyday interactions to established libraries to reduce bespoke maintenance.
Also embed GraphiQL so users can exercise the GraphQL endpoint in a browser without operating a separate UI server.

There are three decisions:

1. Clap definitions are the source of truth for CLI grammar and help. Command processing and diagnostic contracts remain separate.
2. Ratatui renders interactive screens. The existing state machine continues to own approval, cancellation, and candidate installation.
3. Fixed GraphiQL assets are embedded and enabled explicitly through the common
   `Endpoint` builder. Enabling GraphiQL also publishes schema introspection.

Because these interfaces were not released publicly, no compatibility layer is
provided for the handwritten parser, handwritten help, or external pager.
However, not requiring backward compatibility does not mean discarding user files, omitting approval, or allowing changes to security/GraphQL semantics.
The CLI migration does not change generation behavior and must not expose clap,
ratatui, or HTTP-framework types through generated crates.

## 2. Unification of CLI

### 2.1 Target and responsibilities

The initial inventory contains `cargo-pythia`, `pythia-mock`, the books server,
and `production-benchmark`.
At implementation start, inventory argument handling in every binary and example that accepts user input. Do not expose Lab01-owned internal executables.
Do not add unnecessary CLI to examples that do not accept arguments or test fixtures that imitate external commands.
Porting the repository shell script to Rust CLI is not covered.

Derive parsing and help from clap `Command` and `Arg` definitions. Remove
duplicate handwritten grammar and help.
Allow only thin argument normalization, such as removing the initial `pythia` token from a Cargo subcommand invocation; do not retain a second parser.
Internal worker commands are also registered as hidden in the same command tree, and unknown/additional arguments are rejected.
Do not mix human help/progress into existing isolated worker protocols or non-interactive processes.

Use clap's non-exiting parse API and pass the result to existing command
processing; libraries must not call `process::exit`. Receive paths as `OsString`
or `PathBuf`, without panicking or silently applying lossy conversion to non-UTF-8
names. Lower manifest, ownership, and host boundaries reject unsupported input
with their own diagnostics.

### 2.2 Behavior and diagnostics

- Provide `--help`/`-h` for the root and each subcommand, a `help` subcommand, and root `--version`/`-V`.
- Render human help from clap. It need not match old text byte-for-byte, but it must explain required, exclusive, and default values and dangerous effects.
- Preserve existing `GeneratorInfo` fields for version, revision, dirty state, build metadata, and Mock VM version. Do not inspect Git at runtime.
- Reject duplicate singular options, unknown options, missing required values, and exclusivity violations. Preserve repeatable options such as schema and target together with their existing limits.
- Accept clap's standard `--name=value` notation. Do not infer ambiguous abbreviations, ignore unknown options, or forward unknown subcommands externally.
- `cargo-pythia --format human|json` and `pythia-mock reference --format text|json` have different semantics; do not merge them merely because the option names match.
- Write human help to stdout and diagnostics to stderr. Existing JSON diagnostics and version output remain machine-readable and contain no ANSI or help text.
- Usage errors exit 2; help and version exit 0. Preserve existing exit classifications for semantic validation, cancellation, placement, `BrokenPipe`, and other failures.

Raw clap errors may contain user arguments, so never embed them directly in
confidential diagnostics. Derive a safe reason from the error kind and known
options or commands without exposing raw source, provider content, secrets, or
absolute paths. Minimal pre-parse handling may select the diagnostic format, but
must not duplicate option interpretation used for execution.
A help- or version-only invocation must not read manifests or SDL, start a VM, Cargo, or AI, access the network, or modify files.

Preserve manifest-only generation, mutual exclusion of `config-dir` and `config`
for serve and check, non-TTY inference behavior, and apply's mandatory interaction.
Do not add `--yes`, `--force`, unauthorized application, per-value CLI or
environment overrides of manifests, or hidden configuration discovery.

## 3. TUI and reference viewing by ratatui

### 3.1 Separation of drawing and manipulation

Build screens with ratatui `Terminal`, `Frame`, layouts, and widgets. Passing a
handwritten ANSI string through ratatui does not count as migration.
The TUI covers `infer` and `apply` selection, instruction entry, send confirmation, progress, full-text diffs, acceptance and rejection, final approval, and reference viewing. Candidate and original identities, the verified set, approval state, and cancellation and installation transactions remain independent of the rendering library.
One common UI layer owns terminal input and output, raw mode, restoration, the
ratatui backend, and existing bounded I/O. Confine terminal-backend dependencies
to Mock; do not add them to the GraphQL runtime or generator.

The following is required even if the drawing is replaced.

- Preserve every unselected item, require confirmation of hidden selections after search and consent before sending, show a full-text diff for each file, and prohibit approval of unviewed pages.
- Displayed, approved, and placed bytes must equal the candidate formatted and checked under ADR-0034. The UI never reformats, edits, or restamps it.
- Sanitize control characters and invisible content while preserving a reversible source-diff representation and tracking hidden ranges.
- Distinguish bracketed paste and escape sequences from command keys, and discard typeahead between approval stages. Include backend read-ahead in this rule. An incomplete sequence persists across screen boundaries and suppresses commands or edits until it completes. Feed kernel-buffered input through the same decoder; flushing must never discard only a sequence tail. If more than 64 KiB crosses the boundary, restore the terminal and report an environment failure without accepting input.
- Resize and small-screen behavior must not mark hidden content as viewed. Preserve existing screen, queue, and output limits.
- On SIGINT, SIGTERM, cancellation, or terminal I/O failure, collect every job and process and restore terminal mode, flags, cursor, and alternate screen.

Human review has no fixed timeout. Automated processing and terminal writes retain
their existing deadlines. Closing a progress screen never detaches the worker.
Non-TTY `infer` only saves candidates; `apply` still requires a TTY.

### 3.2 Built-in browsing of references

The `ReferenceDocument` source, text and JSON renderings, content, capacity limit,
and `readiness_checked=false` remain unchanged. Display its text rendering in the
built-in ratatui viewer; do not maintain a separate handwritten human format.

| Conditions | Behavior |
| --- | --- |
| Text, stdin and stdout both TTY, without `--no-pager` | Built-in ratatui viewer |
| stdout is pipe/file, or no interactive stdin | direct output of undecorated UTF-8 text |
| `--format json` | Pure JSON document; no viewer or color, even on a TTY |
| `--no-pager` | Direct text output; JSON results are unchanged |
| Terminal initialization failed before starting viewer | Short notice and direct output after restoration |
| Failure after viewer startup | Restore the terminal and return an environment error without writing the main text again |

Provide up/down, PageUp/PageDown, Home/End, `/` search, `n`/`N` next and previous
match, and `q` to quit. Search is literal text search; do not add regular
expressions or external command execution. Remove PAGER and `less` discovery,
process startup, and their dedicated parsing and collection code without a
compatibility fallback.
Shared process management must not be deleted in the name of abolishing external pagers.
Failure to restore the terminal is an environment error, never a successful
direct-output fallback. `BrokenPipe` during JSON or direct text output remains a
normal termination; real I/O failures such as a full disk remain errors.

## 4. GraphiQL configuration and HTTP boundaries

### 4.1 Enabling on a Single Endpoint

The common `Endpoint` builder exposes one `graphiql(bool)` setting in both
typestates, before and after context configuration.
The generated `Resolvers` builder owns resolver registration only. It does not
gain a generated GraphiQL builder or a second schema.
```rust,ignore
let endpoint = pythia::Endpoint::builder(generated_resolvers)
    .context_factory(factory)
    .graphiql(true)
    .build()?;
// The application uses the route helper to integrate with its Router or service.
```
The default is false. Setting true explicitly consents to publication of both the
UI and schema introspection. Document that effect in API rustdoc, help, and usage
examples. This is a narrow addition to ADR-0016's restricted `dangerously_*`
policy, not authorization to replace other safety-release APIs with ordinary
booleans.

Final effective introspection equals the independent introspection setting OR the
final GraphiQL setting. Changing GraphiQL from true to false removes only
GraphiQL-derived disclosure and must not revoke an independent introspection
choice made before or after it. Derive the effective configuration and public
reason from final settings without retaining cancelled reasons or duplicate
overrides.
Setting GraphiQL true once must not irreversibly overwrite the independent
introspection slot. Direct execution, neutral responses, and every bridge from
one endpoint share the same schema and policy.

Publishing GraphiQL does not bypass authentication, CSRF, rate, safelist, cost,
output, or deadline policies. GraphiQL cannot bypass an application policy that
rejects introspection queries. An example shows application-owned middleware
protecting both UI and GraphQL routes without treating UI visibility as GraphQL
authorization.

### 4.2 Explicit route-assembly helpers

Provide neutral `ResponseParts` for HTML and fixed assets. Bind them to the same
enabled state as the `Endpoint`, and compose them with the GraphQL route through
optional Axum and Hyper helpers. The existing handler or service remains
GraphQL-only; `Accept: text/html` never switches it to the UI.

| Default path | Contents |
| --- | --- |
| `/graphql` | Existing GraphQL HTTP contract, including GET and POST |
| `/graphiql` | UI HTML for GET; HEAD returns identical headers without a body |
| `/graphiql/assets/...` | GET and HEAD for assets in the closed inventory only |

When disabled, UI and asset routes are not registered, and neutral response parts
cannot retrieve them from a disabled endpoint. Other methods on a valid UI or
asset path return 405; unknown assets return 404. Neither case reads a GraphQL
body or starts execution. Existing GraphQL method, Accept, and GET-Mutation rules
remain in the GraphQL pipeline.

The helper accepts explicit GraphQL and UI paths and derives the asset prefix and
browser destination from that one route definition.
Each path is a static origin-relative absolute path. Reject schemes, authorities,
queries, fragments, route wildcards, control characters, and collisions.
It provides no external URLs, arbitrary file access, or query-controlled
destination switching.
If an application uses a public prefix, configure it once so HTML and routes
match. Never infer it from `Host` or `Forwarded`. Provide default and custom-
prefix examples without duplicate route entry or implicit adapter correction.

Helpers only assemble a router or service. They do not own listeners, runtimes,
TLS, CORS, signals, shutdown, or readiness. The application applies
authentication and other middleware and mounts the returned routes; a neutral
endpoint owns no route path. Asset responses are separate from in-flight GraphQL
requests and resolvers, and the application still owns whole-server draining.

Public entry points for implementation are `pythia::graphiql::Paths`, `Endpoint::graphiql()`,
`pythia_axum::graphiql::routes`, `pythia_hyper::graphiql::service`.
Enable the `graphiql` feature explicitly in the façade and each bridge. In the
example below, `application_authenticate` represents existing application-owned
authentication middleware.
```rust,ignore
let endpoint = pythia::Endpoint::builder(generated_resolvers)
    .without_context()
    .graphiql(true) // Explicitly expose the UI and schema introspection.
    .build()?;
let paths = pythia::graphiql::Paths::default();
let routes: axum::Router = pythia_axum::graphiql::routes(endpoint.clone(), paths);
let app = routes.layer(axum::middleware::from_fn(application_authenticate));
// The application protects the UI, assets, and GraphQL routes. It owns listening and shutdown.
```
Apply a custom public prefix once in the route definition and merge the result;
do not apply it again with `nest`.
Hyper receives the same path definition. Authentication wraps the complete
service before requests reach `Service::call`.
```rust,ignore
let paths = pythia::graphiql::Paths::new("/query", "/console")?.with_prefix("/api/v1")?;
let routes: axum::Router = pythia_axum::graphiql::routes(endpoint.clone(), paths.clone());
let app = axum::Router::new().merge(routes);
let service = pythia_hyper::graphiql::service(endpoint, paths);
// Both adapters share GraphQL /api/v1/query, UI /api/v1/console, and assets /api/v1/console/assets/….
```
### 4.3 Offline assets and dependencies

Generate GraphiQL HTML, JavaScript, CSS, fonts, and workers from pinned
dependencies and embed them in the distribution binary.
The embedded UI must not depend on a runtime CDN, external fonts, external plugins, or telemetry. Returning HTML alone does not prove offline operation.
The frontend fetcher posts JSON to a configured GraphQL path on the same origin and uses a header that satisfies the existing CSRF contract.
Same-origin authentication cookies may be used, but server environment values
and tokens are never embedded in HTML.
Headers and history remain in memory, and authentication data is never written
to persistent browser storage.
Subscription/WebSocket, original GraphQL client implementation, and optional plugin settings are not added.

Maintain a small frontend entry point, fixed lockfile, update scripts, generated
asset inventory, and license inventory. Regenerate updates with the standard
package manager; CI checks drift and Rust and HTTP product boundaries. Browser
behavior uses the small manual smoke test in Section 7.3 and requires no browser-
automation dependency or CI. Normal Cargo builds, CLI startup, and consumer
compilation require neither Node.js nor asset generation or downloads. Cargo's
selected Git and path dependency acquisition continues to follow ADR-0027; this
feature adds no special Git or SSH capability gate.
Use the generation procedure and inventory to distinguish third-party or minified assets from handwritten frontend glue; generated status must not hide oversized first-party code.
Keep every handwritten JavaScript, TypeScript, CSS, and HTML file within 400
lines and include those extensions explicitly in the rules checker.

Isolate assets in an optional crate. The façade and bridge `graphiql` features
default off and Mock enables them explicitly. Consumers with the feature disabled
must not link the assets. Even when compiled with the feature, runtime
`graphiql(false)` remains the default.
Core retains zero normal dependencies and runtime neutrality. Ordinary products
remain on Rust 1.85 and Mock on Rust 1.95.
WP-DX35-0 verifies and pins compatible clap, ratatui, terminal-backend, and
GraphiQL versions and records Cargo and npm lockfiles and evidence.
Run dependency license and advisory checks. Any additional license exception or MSRV change requires a separate explicit decision.

As a Rust 1.95 maintenance correction on 2026-09-09, nine ratatui 0.29.0 return
types were changed to spell their inferred lifetime as `'_`. Preserve the
vendor's original distribution hash and verify both the narrow patch and the
fully reapplied bytes. Runtime behavior, dependency versions, licenses, and lint
configuration do not change. The
[vendor information](../../vendor/README.md) records the patch scope and
reapplication procedure; the
integrated validation record
records its verification.

On 2026-09-09, the user explicitly approved `entities 4.5.0` (BSD-2-Clause), `tslib 2.8.1` (0BSD), and `argparse 2.0.1` (Python-2.0).
Only these three package, version, and license triples are recorded as narrow,
user-approved exceptions in `crates/graphiql/license-policy.json`.
Preserve the base allowlist, inspection of all acquired dependencies, and original LICENSE and notice files. This approval is distinct from the MPL-2.0 approval.

Settings inserted into HTML are escaped according to the JSON/HTML context, and only fixed assets are delivered.
Set `Content-Type`, `X-Content-Type-Options: nosniff`, and CSP. Do not make offline
tests pass by relaxing CSP or permitting external sources. Scripts may use only
embedded same-origin assets, with style and worker permissions minimized for the
actual bundle.
Prevent user- or schema-derived strings from executing as script or HTML. Rust and HTTP tests cover configuration escaping, CSP, and asset responses. Review frontend configuration and source for safe rendering and disabled persistent storage; a new comprehensive browser attack suite is out of scope.

## 5. Mock usage

```bash
pythia-mock serve --config-dir mock --enable-graphiql
pythia-mock serve --config single.toml --enable-graphiql
```
`--config-dir` and `--config` are mutually exclusive. `--enable-graphiql` is a
single-use, `serve`-only flag; duplicates are rejected.
Without the flag, UI and introspection remain hidden; with it, both are enabled.
The setting is not stored in projects or portable bundles and does not propagate
to export, infer, or reference. Init, check, update, and export neither open a
browser nor start a server.

Bind only after existing configuration, type, seed, relation, and readiness checks
pass, and retain the loopback restriction. Display the actual bound GraphQL URL
at startup. With the flag, also display the UI URL and a schema-publication notice.
Do not open a browser automatically or fall back silently to another free port.

Existing Mock configurations allow either no origins or the exact
`allowed_origins` set. With GraphiQL enabled, runtime may additionally allow only
its own HTTP origin derived from the actual numeric loopback address and port.
For port 0, use the assigned port after binding. Format IPv6 with brackets and do
not add implicit localhost aliases.
Do not trust the request `Host` to expand permissions. Continue to reject null,
duplicate, foreign, and otherwise invalid origins.
Without the flag, existing `allowed_origins` semantics remain unchanged. Do not
introduce wildcards or credentialed cross-origin disclosure.
This narrow origin addition belongs to Mock; the general endpoint and helpers do
not own a CORS policy.

GraphQL retains existing worker admission, IR and type checks, read and write
scope, root Mutation transactions, cancellation, and draining. Asset requests
start no VM and invoke no AI inference. GraphiQL operations use the same worker
and endpoint as ordinary GraphQL requests.

## 6. Migration and non-goals

Record the baseline after ADR-0034 implementation acceptance and preserve all
untracked results from parallel work. When both ADRs share one sequential goal,
focused completion of ADR-0034 WP-FMT0 through WP-FMT3 may serve as the
intermediate checkpoint. Consolidate expensive whole-suite verification and final
independent Reviews A and B after integration, and confirm every FMT and DX35 ID
against final source without marking ADR-0034 complete early.
This intermediate checkpoint requires neither external designer review nor a
commit or distribution gate. Do not change ABI 6, `SchemaDigest`, the generated
resolver contract, project v1, bundle v3, API v2, or reference v2.
Proposal v2, formatted candidates, B/U/N, comparison originals, recovery and
user edits, and concurrent-edit fencing continue to follow ADR-0032 through
ADR-0034. Migrate examples, tests, and scripts that depend on CLI arguments or
help; do not retain the old parser or PAGER as compatibility mechanisms.

GraphiQL is opt-in; it does not enable existing consumers or modify external applications.
Publisher identity, SSH acquisition, package release, publication, tags, and new
real-AI approval are not completion conditions.
Treat reproducible GraphiQL asset updates as an ordinary product check; do not
restore obsolete distribution-only pipelines.

## 7. Implementation order and acceptance

### 7.1 Work packages

1. WP-DX35-0: Capture the baseline and ADR-0034 state; inspect CLI, terminal, HTTP, and dependency inventories, library conformance, and asset-bundle policy.
2. WP-DX35-1: Connect clap definitions to existing processing and diagnostics,
   remove handwritten parsing and help, and migrate examples and benchmarks.
3. WP-DX35-2: Implement ratatui rendering, shared terminal ownership, the built-in reference viewer, existing machine-readable behavior, and PTY regression tests.
4. WP-DX35-3: Integrate optional embedded assets, neutral responses, endpoint
   settings and snapshots, Axum and Hyper helpers, and generated-consumer examples.
5. WP-DX35-4: Integrate the Mock flag, exact runtime origin, startup display,
   existing HTTP checks, and a manual procedure using one browser and fixture.
6. WP-DX35-5: Complete common validation, measurements, independent Reviews A
   and B, correction and re-review of every finding, and usage, migration, and
   verification records.

Run focused tests during implementation and reserve expensive shared full verification for the final source. Reuse caches and do not delete unrelated targets.
Run the 400-line checker after every subtask, after formatting, and before
handoff. Regenerate owned products through their normal procedures rather than
editing generated files by hand.
No real AI calls are required, and inference TUI verification is limited to fake backend/executables and dedicated fixtures.

### 7.2 Closed acceptance inventory

Initialize every ID as Required / Not run. Map automated evidence to concrete
tests, individual assertions, commands, and results. DX35-21 must either be
verified manually or handed to the user with exact instructions. Keep
implementation acceptance and UI confirmation separate as required by Section
7.3.

| ID | Required confirmation |
| --- | --- |
| DX35-01 | Inventory every CLI, binary, and argument-taking example; define parsing and help once in clap; remove handwritten and hidden secondary parsers. |
| DX35-02 | Verify root and all subcommand help, short help, version, `GeneratorInfo`, VM version, exit status, and absence of settings, VM, communication, or write side effects. |
| DX35-03 | Cover unknown, missing, duplicate, conflicting, repeated, and over-limit input; `--name=value`; Cargo and direct invocation; and non-UTF-8 paths without panic. |
| DX35-04 | Preserve human and JSON output, stdout and stderr placement, and exit classification while excluding raw arguments, secrets, absolute paths, and ANSI from parse-failure diagnostics. |
| DX35-05 | Preserve manifest-only generation, config-source exclusivity, non-TTY infer, interactive apply, the closed hidden-worker protocol, and existing command behavior. |
| DX35-06 | Verify library, MSRV, and license compatibility; correct placement of CLI and asset dependencies; default features; and absence of unnecessary core, runtime, or generated dependencies. |
| DX35-07 | Render selection, instructions, send consent, progress, diffs, acceptance, and final approval with ratatui, verified by `TestBackend` and a real PTY. |
| DX35-08 | Cover long full text, line breaks, controls, non-ASCII text, unviewed pages, hidden selections, small terminals, resize, approval, and rejection. |
| DX35-09 | Cover split escape sequences, paste, and typeahead that cannot approve; candidate display, saved bytes, and placed bytes must match. |
| DX35-10 | Cover cancellation, signals, terminal failure, output backpressure, worker collection, and restoration of mode, flags, cursor, and alternate screen. |
| DX35-11 | Cover reference scrolling, search, next and previous match, end navigation, pre-start fallback, post-start failure, and absence of command execution from `PAGER`. |
| DX35-12 | Cover reference TTY, pipe and file output, noninteractive stdin, JSON, and `--no-pager`; preserve body bytes and distinguish `BrokenPipe` from real I/O failure. |
| DX35-13 | Pin GraphiQL assets and lockfile; close inventory and licenses; prove reproducible regeneration; and require no Node.js or download for normal Cargo builds or startup. |
| DX35-14 | Cover neutral HTML and assets, `Content-Type`, HEAD, 405, 404, CSP, escaping, unknown paths, arbitrary-file refusal, and external-destination refusal. |
| DX35-15 | Cover both builder typestates through false, true, and re-disabled settings, independent introspection in both call orders, and correct final snapshot and reason. |
| DX35-16 | Cover Axum and Hyper with default and custom prefixes, path collisions, absent UI and assets when disabled, and unchanged GraphQL responses from existing handlers. |
| DX35-17 | Prove that authentication, CSRF, rate, safelist, deadline, and output limits apply through GraphiQL and demonstrate application-owned authentication outside the UI route. |
| DX35-18 | Generated `Resolvers` and the owned Mock schema use the same builder; no second schema or generated HTTP builder is introduced, and ABI remains unchanged. |
| DX35-19 | Cover both Mock config forms with and without the flag, reject duplicate and conflicting specifications, preserve export and config bytes, and retain pre-start readiness checks. |
| DX35-20 | Do not loosen CORS: cover numeric IPv4 and IPv6, port 0, the exact own origin, no flag, foreign and null origins, duplicates, and fake hosts. |
| DX35-21 | Perform the minimum UI, completion, Query, and Mutation smoke test without external requests, or hand off reproducible steps for one browser and existing fixture; otherwise record manual confirmation as pending. |
| DX35-22 | Reuse Rust and HTTP tests for introspection, variables, post-Mutation state, and GraphQL errors; do not duplicate two-domain, alias, or fragment coverage in the browser. |
| DX35-23 | Never embed queries, variables, or headers in server HTML. Verify escaping and CSP with Rust and HTTP tests, and review frontend settings and source for disabled persistence and safe schema rendering. |
| DX35-24 | Preserve HTTP, worker, Mutation rollback, cancellation, and drain with or without the UI; serving or acquiring assets never invokes AI or process inference. |
| DX35-25 | Common validation covers Rust 1.85, Mock 1.95, stable Rust, all-SDK exactness and quality, ABI 6, `SchemaDigest`, and feature-on and feature-off compilation. |
| DX35-26 | Preserve ADR-0034 candidate and approval bytes, originals, U, B, `.dist`, export, recovery, and existing VM stop, re-entry, and return behavior. |
| DX35-27 | Record binary and asset sizes and comparable cold build, rebuild, and startup conditions before and after. No browser-performance platform is required, and unmeasured behavior is never claimed as improved. |
| DX35-28 | Verify handwritten versus asset ownership, 400-line and English-prose rules, removal of old routes, dependency and license policy, both diff checks, and cache reuse. |
| DX35-29 | Independent Reviews A and B correct and re-review every reasonable-severity finding and report remaining findings separately from designer judgment. |
| DX35-30 | Record every ID, usage, features, disclosure warnings, migration procedures, important untracked and recovery material, unexecuted OS and remote CI, and UI handoff pending manual confirmation. |

Gate DX35-A is 01-06, DX35-B is 07-12, DX35-C is 13-18, DX35-D is 19-24, and DX35-E is 25-30.
Review A independently checks CLI/TUI/approval/ownership/0034 regression, and Review B independently checks HTTP/GraphiQL/dependencies/assets/browser safety boundaries.
Implementation gates may pass when automated checks and an exact manual procedure
are complete; pending DX35-21 UI confirmation is not an implementation blocker.
Do not report HTML or HTTP test success as browser confirmation. Record
implementation acceptance and the separate manual-UI result together. Keep local
Linux and macOS results, remote CI, and designer review distinct, and never count
unexecuted or skipped work as Passed. Staging, commits, pushes, publication, and
tags are not completion conditions.

### 7.3 Minimal browser confirmation and manual handoff

The purpose is to confirm that Pythia's embedded GraphiQL is usable, not to
reimplement GraphiQL's own end-to-end suite. With one existing bootable Mock
fixture and the user's normal browser, check only these points:

1. Start with `--enable-graphiql` and open the displayed `/graphiql` URL. Confirm
   that the UI renders and the DevTools Network panel shows no external-origin
   requests.
2. Browse the schema and use field completion to run one query with variables.
3. Run one Mutation and confirm that a later Query reflects the change.

Confirm the absence of external requests by reloading with the browser cache disabled. A dedicated OS-level network-interruption harness is not required.
Automated checks continue to cover enabled and disabled UI and assets, HTTP
headers, escaping, paths, introspection, Query, Mutation, and origins.
Reuse existing tests and helpers; do not add large fixtures, dedicated servers,
or general-purpose E2E runners solely for the manual smoke test.
Playwright, Selenium, automatic browser downloads, browser-specific CI, a
browser/OS/domain matrix, browser performance measurements, and comprehensive
GraphiQL editor coverage are outside the required scope. Real-PTY approval and
restoration checks remain mandatory.

The handoff provides the fixture preparation and start command, access URL,
Query, Mutation, variables, and expected results. Keep the procedure in existing
usage and verification records rather than creating a separate automated product
or large fixture set. If the implementer cannot perform the browser check, the
implementation may still be handed off after automated checks and the procedure
are complete. Record that state as "Implementation acceptance Passed / Manual UI
confirmation pending" and do not claim browser success. Add a browser, date, and
result only after actual confirmation.

## 8. Documentation and evidence

This ADR is the primary DX35 contract. Keep README, architecture, handoff, and
conformance references and status synchronized. ADR-0004, ADR-0008, ADR-0016,
and ADR-0031 record the corresponding narrow amendments without rewriting their
historical implementation results as new acceptance. Operation examples, Cargo
features, schema-disclosure warnings, and DX35 evidence are complete. The common
84-command suite, measurements, independent corrections, and re-reviews passed,
so local implementation acceptance passed. Individual gate results and designer
review are aggregated in the
integrated validation record.
Manual UI confirmation and remote CI remain unexecuted and are distinct from
designer-review findings.

- [clap official API](https://docs.rs/clap/latest/clap/) / [source](https://github.com/clap-rs/clap)
- [Ratatui official materials](https://ratatui.rs/) / [source](https://github.com/ratatui/ratatui)
- [GraphiQL official introduction material](https://github.com/graphql/graphiql/blob/main/packages/graphiql/README.md)
- [ADR-0004](0004-http-adapters.md) / [ADR-0016](0016-single-endpoint-builder-and-safe-baseline.md)
- [ADR-0031](0031-mock-model-operations-and-generated-reference.md) / [ADR-0034](0034-generated-luau-formatting.md)
