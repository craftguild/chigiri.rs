"""Select the explicit ADR-0058 validation scope without dropping new checks."""

from dataclasses import replace


PROFILES = ("ci", "full")
DEFAULT_PROFILE = "ci"
COMPLETION_SCHEMA = "chigiri/product-validation-v2"

# Broad/package checks are deferred to weekly or requested full runs.
# Both profiles use the same fixed Rust 1.95 product toolchain.
FULL_ONLY = frozenset({
    "host-package", "chigiri-relocated-acceptance",
    "generated-quality-1.95.0", "full-schema-consumer-1.95.0",
    "large-consumer-conformance",
    "mock-model-unit-release-1.95.0", "mock-model-http-release-1.95.0",
    "mock-project-http-release-1.95.0", "mock-project-vm-release-1.95.0",
    "mock-reference-release-1.95.0", "mock-vm-spike-release-1.95.0",
    "mock-graphiql-release-1.95.0", "mock-hooks-http-release-1.95.0",
    "mock-all-http-release-1.95.0",
})


def select(profile, steps):
    if profile not in PROFILES:
        raise ValueError("The validation profile must be ci or full.")
    if profile == "full":
        return steps
    if not FULL_ONLY.issubset({step.label for step in steps}):
        raise ValueError("The deferred validation catalog changed without a policy update.")
    selected = []
    for step in steps:
        if step.label in FULL_ONLY:
            continue
        if step.label == "check-production-performance-harness":
            step = replace(step, argv=(*step.argv, "1.95.0"))
        selected.append(step)
    return selected
