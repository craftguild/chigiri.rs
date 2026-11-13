---
name: craftguild-standardize
description: Apply this repository's fixed Craftguild engine and matching rules.
---

Run `make prime` before adoption work. If Make wiring is unfinished, run
`python3 standards/launcher.py prime`. Read relevant details with
`make prime PRIME_ARGS="--topic NAME"`. The fixed engine owns the rules;
do not substitute an installed tool's version. Missing setup requires explicit
`make setup`. Keep the engine pin and common-crate Git revisions consistent.

Inspect product instructions, design, dirty work and existing coverage first.
Standards owns common engine, font and measurement tests. Products retain SDK,
ABI, generator, domain and actual integration checks. Remove retired adapter
tests after identifying their upstream owner; do not move them into runtime
crates or retain figlet-rs/vendor solely to test standards. Add product tests
only for a concrete uncovered connection risk. Report generic measurement gaps
upstream. Preserve product behavior, required gates and unrelated work.

Use `craftguild-standards init` to update explicitly to the invoking initializer's
published version/revision. Downgrades and same-version revision replacement
require `--force`; edits, integrity and compatibility checks still apply.
`make setup` restores only the current pin, and ordinary operations never update.
After updating, manually align common-crate Git revisions with the reported full
revision and adapt product configuration/APIs before product checks.

Initialization is unfinished adoption. Configure real artifacts, source roles
and gates, then run `make check` and artifact acceptance. Publication, product
migrations, installation and service operations require the active request's
authorization. Never claim an unexecuted check or host qualification passed.
