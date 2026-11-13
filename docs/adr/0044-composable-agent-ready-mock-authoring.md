# ADR-0044: Composable agent-ready mock authoring

Status: Accepted

Date: 2026-09-18

Implementation: Implemented

Planned successor: [ADR-0051](0051-mock-contract-validation-without-completion-state.md)
removes completion declarations and unfinished tracking at implementation
cutover. Full readiness remains executable-contract validation, not a claim of
business completeness. Scoped checks, generated guidance, reference discovery,
and external agent ownership remain supported; current operating behavior is
unchanged until that implementation lands.

## Context

Pythia Mock initially embedded an agent orchestration system through `infer` and
`apply`. It selected targets, assembled all context in advance, invoked Codex,
parsed a closed candidate protocol, retried invalid candidates, stored proposals,
rendered a custom review TUI, and installed approved files atomically.

That design provided a strong property: the agent did not receive write access
to the active project. It also duplicated capabilities already present in coding
agents, including file discovery, scoped editing, command execution, model
selection, timeouts, diff review, and recovery through version control.

The dedicated inference implementation, proposal/adoption implementation, and
integration tests account for approximately 16,400 lines, excluding shared TUI
code, scripts, and design records. It also couples Pythia Mock to Codex CLI event
and invocation details. Large schemas require Pythia to preassemble substantial
context even when an agent could discover only the relevant files on demand.

The mock project already has the primitives an external agent needs:

- an explicit editable directory;
- generated Luau declarations;
- a schema-derived model API reference;
- deterministic static and readiness checks; and
- ordinary files that can be reviewed with the user's source-control tools.

## Decision

Pythia Mock will be agent-ready rather than an agent orchestrator. External
agents own reasoning, model configuration, file editing, and interaction. Pythia
owns project generation, scoped reference data, deterministic validation, and
runtime readiness.

This follows a composable command model: each command emits stable output,
performs one bounded operation, and can be invoked by a human, shell script, or
agent without a Pythia-owned conversation session.

### Generated agent guidance

`init` writes `AGENTS.md` at the project root. The document is useful to Codex
and readable by other agents. It states:

- the normal authoring sequence;
- directly editable resolver, seed, and hook paths;
- conditional ownership of `mock.luau`;
- read-only generated metadata and external SDL paths;
- scoped `reference` commands; and
- target and whole-project `check` commands.

`AGENTS.md` is advisory input, not a sandbox or execution authority. It is
created with a new project, excluded from portable bundles, and is not part of
runtime readiness. The user controls the external agent's filesystem, network,
authentication, model, and tool permissions.

### Scoped reference output

The existing topic command is extended with schema-derived `types` and
`resolvers` topics. The following commands return bounded text or JSON without
checking readiness:

```text
pythia-mock reference types TYPE --config-dir DIR --format json
pythia-mock reference models MODEL --config-dir DIR --format json
pythia-mock reference resolvers PARENT.FIELD --config-dir DIR --format json
```

`types` is selected by exact GraphQL type name. `models` retains its existing
exact model-name selection. `resolvers` is selected by exact GraphQL coordinate.
Unknown names fail as usage errors without echoing untrusted argument values.
Reference output is written directly to stdout; terminal paging and interaction
belong to normal Unix tools.

### Machine-readable and targeted checks

`check` accepts `--format text|json`, defaulting to `text`. JSON uses the closed
format `pythia-mock-check/v1` and reports the scope, validity, readiness state,
selected targets, and sanitized diagnostics.

`--target PATH` may be repeated with `--config-dir`. Each value must be an exact
active resolver, seed, or enabled-hook path. Absolute paths, traversal, globs,
generated files, configuration, SDL, disabled hooks, duplicate paths, and
unknown files are rejected.

Target checking validates the current bytes of the selected files with the same
schema-derived Luau and seed contracts used by full readiness. Related model
seeds are validated as one connected component. It does not execute resolvers.

A valid target set exits zero even when unrelated files keep the project from
starting. Its result then has `valid=true` and `readiness_checked=false`. A full
project check exits zero only with `readiness_checked=true`. Environment or
resource failures never become target success.

Portable bundles do not support `--target` because they are immutable runtime
artifacts rather than editable authoring projects.

### Removal of integrated inference

The unreleased `infer` and `apply` commands, Codex adapter, inference protocol,
proposal format, adoption flow, communication viewer, diff/editor TUI, and
isolated inference worker are removed. No compatibility reader or deprecated
alias is retained.

The interactive reference viewer is also removed. `reference` writes to stdout,
supports broken-pipe success, and composes with tools such as `less`, `jq`, and
file redirection. This removes the terminal UI dependency from Pythia Mock.

Historical ADRs and validation records continue to describe the removed design
at the revisions where it existed. Their inference decisions are superseded by
this ADR for the current product contract.

## Agent workflow

The supported workflow is:

1. Run `init` and open the generated project as the agent workspace.
2. Let the agent read `AGENTS.md`, SDL, generated types, and scoped references.
3. Edit only the files required by the user request.
4. Run `check --target` while other resolver or seed work remains incomplete.
5. Run full `check` as the final authority.
6. Start `serve` only after full readiness succeeds.

Pythia does not infer that an agent-generated implementation has correct
business meaning. Users review changes and exercise representative GraphQL
operations just as they do for handwritten implementations.

## Consequences

The main binary and repository lose agent-specific process integration,
proposal storage, terminal interaction, and a vendored terminal UI dependency.
The remaining interfaces work with Codex and other coding agents without a new
backend adapter.

External agents may edit files directly. Users who require a no-write agent
boundary must provide it through their agent sandbox, a copy, a worktree, or
source control. Pythia still prevents `check` and `serve` from accepting invalid
project state.

Machine-readable diagnostics and scoped references become public formats and
require normal versioning discipline. Target success must remain distinct from
whole-project readiness in every output mode.

## Rejected alternatives

Keeping both complete workflows would preserve choice but retain almost all
maintenance cost and leave two competing onboarding paths.

Moving `infer` to another crate would reduce the main binary surface but not the
repository maintenance or Codex CLI coupling.

Generating only a prompt would still require users and agents to scrape the
full reference and human diagnostics. Scoped reference data and JSON checks are
general-purpose primitives and provide more durable value.

## Implementation and verification

Implementation is complete when:

- new projects contain the bounded English `AGENTS.md` guidance;
- type, model, and resolver references work in text and JSON;
- full and targeted checks work in text and `pythia-mock-check/v1` JSON;
- target validation cannot claim whole-project readiness;
- `infer`, `apply`, proposal/adoption code, terminal UI code, and their tests are
  absent from the active build and validation plan;
- unused terminal dependencies and vendored sources are removed;
- the README demonstrates direct external-agent authoring; and
- the repository rules, formatting, focused tests, workspace tests, and exact
  dependency checks pass.
