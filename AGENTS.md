<!-- craftguild:common:start -->
Use Craftguild standards 0.1.0 with the fixed engine in standards/engine.json.
Run `make prime` (or `python3 standards/launcher.py prime`) before adoption.
Read relevant topics with `make prime PRIME_ARGS="--topic NAME"`.
Follow the repo-local craftguild-standardize Skill. Run `make check`.
Update explicitly with `craftguild-standards init [--force]`; follow its dependency revision guidance.
<!-- craftguild:common:end -->

## Shared agent files

`AGENTS.md` is the repository instruction authority. The two shared skills in
`.agents/skills/` cover Beads and the pinned Craftguild entry. Tool-specific
`.claude/`, `.codex/`, `.cursor/` and editor settings are local and ignored.
Beads data and configuration stay entirely local in ignored `.beads/`; do not
publish or synchronize them. Run `bd prime` for the local task workflow.

# chigiri.rs contributor instructions



## Product contracts

Read the design authorities in [docs/README.md](docs/README.md) before changing
implementation, in its stated order. Update the relevant design document in the
same change when a decision differs. ADR-0059 amends common integration,
measurement, build/release entries and licensing; earlier historical results
retain their original scope.

Preserve generated ABI, closed manifests/formats, complete generated ownership
and isolated exact regeneration, bounded resource and filesystem/process
protection, std-only core, the uniform Rust 1.95 build/SDK minimum and normal/Mock dependency boundary,
and the native/distribution platform tiers. Do not hand-edit generated output.
Keep first-party prose in English under ADR-0043, including its one literal
README name-origin exception.

Product CLI artifact scenarios and assertions live in Rust integration tests.
`make acceptance` explicitly runs the opt-in test against the verified built
artifact; an ignored default-suite result does not qualify acceptance. Scripts
only prepare its environment and invoke commands. Keep test compilation separate
from the delivered artifact; infrastructure tests retain their own implementation
language and adopted frameworks retain their standard test tooling.

Run `./scripts/check-implementation-rules.sh` before work. Use its
`--product-only` checks after a subtask and formatting. At integration/handoff,
`make check` owns the shared assessment once and product boundary checks before
and after the catalog. The first boundary retains the product ownership converter;
the final `--boundaries-only` check does not repeat it. Resolve failures before the next subtask.
[Implementation rules](docs/implementation-rules.md) and the
[handoff](docs/implementation-handoff.md) define the working gates.

## Task and review tracking

Use Beads for all tasks and reviews. Read the
[Beads skill](.agents/skills/beads/SKILL.md), then run `bd prime`.
Claim work before editing. Keep issue IDs and legacy finding IDs stable.
Follow [review workflow](docs/review-workflow.md): implementers record Fixed;
independent reviewers alone record Verified and close findings. Tests use
normative contracts and explicit fixtures, never review states or Beads data.
Do not create Markdown task/review ledgers or ad hoc memory files.

The conservative profile applies: no Git staging, commits, pushes or Dolt
remote sync without explicit authority. A current user restriction takes
precedence. Sign every authorized Git commit with OpenPGP and verify its full
revision; stop on a real signing or verification failure. This policy applies
only to separately authorized repository commits. Ordinary tests and checks
must not create Git commits or signing keys, invoke signing, or require GPG.
Use explicit repo-owned Cargo metadata fixtures for Git source identities;
do not manufacture repository history merely to run validation.
