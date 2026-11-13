"""Define the mandatory product-validation command set without publisher mode."""

from dataclasses import dataclass

from validation_profiles import select

NORMAL_PACKAGES = (
    "chigiri_axum", "chigiri_codegen", "chigiri_core", "chigiri_host", "chigiri", "chigiri_http",
    "chigiri_hyper", "chigiri_graphiql", "chigiri_observability", "chigiri_opentelemetry", "chigiri_operation",
    "chigiri_policy", "chigiri_runtime", "chigiri_rate", "chigiri_rate_memory",
    "chigiri_testkit", "chigiri_tracing", "chigiri_axum_adoption", "chigiri_books",
    "chigiri_subscriptions_example",
)

MOCK_TEST_TARGETS = (
    "module_vm", "project_vm", "runtime_vm", "vm_spike",
)

CLI_TEST_TARGETS = (
    "brand_cutover", "command_tree", "host_writer", "mock_cli", "mock_reference", "rust_builtin_imports",
    "rust_consumer", "rust_contract", "rust_diagnostics", "rust_doctor",
    "rust_enum_binding", "rust_generation", "rust_traits", "rust_trait_errors",
    "rust_terminal", "rust_version",
)


@dataclass(frozen=True)
class Step:
    label: str
    argv: tuple[str, ...]
    seconds: int = 600


def mock_tests(toolchain: str) -> list[Step]:
    # Keep VM targets bounded; Cargo test compiles the CLI targets in one invocation.
    cargo = ("cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_mock", "--all-features")
    cli = ("cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_cli", "--all-features")
    # Bound VM and filesystem fixture pressure across independent unit cases.
    aggregate = (*cargo, "--lib", "--examples", "--benches", "--", "--test-threads=1")
    return [
        Step(f"mock-test-{toolchain}", aggregate, 1800),
        *(Step(f"mock-test-{name}-{toolchain}", (*cargo, "--test", name), 1800)
          for name in MOCK_TEST_TARGETS),
        Step(f"cli-tests-{toolchain}", (
            *cli, *(arg for name in CLI_TEST_TARGETS for arg in ("--test", name)),
        ), 1800),
        Step(f"cli-mock-doctest-{toolchain}", (
            "cargo", f"+{toolchain}", "test", "--locked", "-p", "chigiri_mock",
            "-p", "chigiri_cli", "--all-features", "--doc",
        ), 1800),
    ]


def plan(profile: str = "full") -> list[Step]:
    steps = [
        Step("implementation-rules", ("./scripts/check-implementation-rules.sh",
             *(("--product-only",) if profile == "ci" else ()))),
        Step("implementation-rules-regression", ("./scripts/test-implementation-rules.sh",)),
        Step("host-boundary-regression", ("./scripts/test-host-boundary.sh",)),
        Step("host-manifest", ("bash", "scripts/python-with-toml.sh", "scripts/check-host-manifest.py")),
        Step("host-manifest-regression", (
            "bash", "scripts/python-with-toml.sh", "scripts/test-host-manifest.py",
        )),
        Step("host-package", (
            "cargo", "+1.95.0", "package", "--locked", "-p", "chigiri_host", "--allow-dirty",
        ), 1800),
        Step("chigiri-brand", ("bash", "scripts/python-with-toml.sh", "scripts/check-chigiri-brand.py")),
        Step("chigiri-brand-regression", ("python3", "scripts/test-chigiri-brand.py")),
        Step("sibling-repository", ("python3", "scripts/sibling_repository.py")),
        Step("sibling-repository-regression", ("python3", "scripts/test-sibling-repository.py")),
        Step("sibling-acceptance-trace", ("python3", "scripts/sibling_acceptance_trace.py")),
        Step("sibling-acceptance-trace-regression", ("python3", "scripts/test-sibling-acceptance-trace.py")),
        # Build already prepares and verifies its inputs before compilation.
        Step("chigiri-build", ("sh", "tools/standards.sh", "build", "--artifact", "chigiri-cli", "--profile", "debug")),
        Step("chigiri-relocated-acceptance", ("python3", "scripts/chigiri-relocated-acceptance.py"), 1800),
        Step("generated-exact", ("bash", "scripts/python-with-toml.sh", "scripts/check-generated-crates.py", "exact")),
        Step("generated-surface", ("bash", "scripts/python-with-toml.sh", "scripts/check-generated-surface.py")),
        Step("workspace-format", ("cargo", "+1.95.0", "fmt",
             *(arg for package in NORMAL_PACKAGES for arg in ("-p", package)), "--", "--check")),
        Step("cli-mock-format", ("cargo", "+1.95.0", "fmt", "-p", "chigiri_mock", "-p", "chigiri_cli", "--", "--check")),
        Step("mock-contract", ("python3", "scripts/check-mock-contract.py")),
        Step("subscription-recipes", ("./scripts/check-subscription-recipes.sh",)),
        Step("vendored-sources", ("bash", "scripts/python-with-toml.sh", "scripts/vendored_sources.py")),
        Step("vendored-sources-regression", ("bash", "scripts/python-with-toml.sh", "scripts/test-vendored-sources.py")),
        Step("graphiql-assets-inventory", ("python3", "scripts/graphiql_inventory.py")),
        Step("graphiql-assets-regression", ("python3", "scripts/test-graphiql-inventory.py")),
        Step("graphiql-assets-drift", ("python3", "scripts/graphiql_build.py", "--check", "--typecheck")),
        Step("graphiql-consumer-format", ("cargo", "+1.95.0", "fmt", "--manifest-path", "tests/fixtures/graphiql-consumer/Cargo.toml", "--", "--check")),
        Step("graphiql-consumer-feature", ("python3", "scripts/check-graphiql-contract.py")),
        Step("full-schema-domain-format", ("cargo", "+1.95.0", "fmt", "--manifest-path", "tests/fixtures/full-schema/domain/Cargo.toml", "--", "--check")),
        Step("full-schema-source-corpus", ("bash", "scripts/python-with-toml.sh", "scripts/full_schema_corpus.py")),
        Step("full-schema-source-regression", ("bash", "scripts/python-with-toml.sh", "scripts/test-full-schema-corpus.py")),
        Step("fuzz-format", ("cargo", "+1.95.0", "fmt", "--manifest-path", "fuzz/Cargo.toml", "--", "--check")),
    ]
    for toolchain in ("1.95.0",):
        # Clippy and test already compile every target. A separate build repeats
        # build-script observation and relinking without adding a test scenario.
        for operation in ("clippy", "doc"):
            argv = (
                "cargo", f"+{toolchain}", operation, "--locked", "-p", "chigiri_mock",
                "-p", "chigiri_cli", "--all-features",
            )
            if operation == "doc":
                argv = ("env", "RUSTDOCFLAGS=-Dwarnings", *argv, "--no-deps")
            else:
                argv += ("--all-targets",)
                if operation == "clippy":
                    argv += ("--", "-D", "warnings")
            steps.append(Step(f"cli-mock-{operation}-{toolchain}", argv, 1800))
        steps.extend(mock_tests(toolchain))
    steps.append(Step("mock-quickstart", (
        "python3", "scripts/mock-quickstart.py",
    )))
    steps.append(Step("chigiri-prepare-release-1.95.0", (
        "sh", "tools/standards.sh", "prepare", "--artifact", "chigiri-cli", "--profile", "release",
    )))
    steps.append(Step("mock-reentry-release-1.95.0", (
        "cargo", "+1.95.0", "test", "--locked", "--release", "-p", "chigiri_cli",
        "--all-features", "--test", "mock_cli", "reentry::", "--", "--nocapture",
    ), 1800))
    steps.append(Step("mock-runtime-vm-release-1.95.0", (
        "cargo", "+1.95.0", "test", "--locked", "--release", "-p", "chigiri_mock",
        "--all-features", "--test", "runtime_vm", "--", "--nocapture",
    ), 1800))
    for label, package, targets in (
        ("mock-model-unit-release-1.95.0", "chigiri_mock", ("--lib",)),
        ("mock-model-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "runtime_schema::model_api::")),
        ("mock-project-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "project_")),
        ("mock-project-vm-release-1.95.0", "chigiri_mock", ("--test", "project_vm", "--test", "module_vm")),
        ("mock-reference-release-1.95.0", "chigiri_cli", ("--test", "mock_reference")),
        ("mock-vm-spike-release-1.95.0", "chigiri_mock", ("--test", "vm_spike")),
        ("mock-graphiql-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "graphiql::")),
        ("mock-hooks-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli", "hk_")),
        ("mock-all-http-release-1.95.0", "chigiri_cli", ("--test", "mock_cli")),
    ):
        steps.append(Step(label, (
            "cargo", "+1.95.0", "test", "--locked", "--release", "-p", package,
            "--all-features", *targets, "--", "--nocapture",
        ), 1800))
    for toolchain in ("1.95.0",):
        steps.append(Step(f"rustc-version-{toolchain}", ("rustc", f"+{toolchain}", "--version", "--verbose")))
    for toolchain in ("1.95.0",):
        cargo = ("cargo", f"+{toolchain}")
        for operation in ("clippy", "test", "doc"):
            argv = (
                *cargo, operation, "--locked", "--workspace", "--all-features",
                "--exclude", "chigiri_mock", "--exclude", "chigiri_cli",
            )
            if operation == "doc":
                argv = ("env", "RUSTDOCFLAGS=-Dwarnings", *argv, "--no-deps")
            else:
                argv += ("--all-targets",)
                if operation == "clippy":
                    argv += ("--", "-D", "warnings")
            steps.append(Step(f"workspace-{operation}-{toolchain}", argv, 1800))
            if operation == "test":
                steps.append(Step(f"workspace-doctest-{toolchain}", (
                    *cargo, "test", "--locked", "--workspace", "--all-features",
                    "--exclude", "chigiri_mock", "--exclude", "chigiri_cli", "--doc",
                ), 1800))
        steps.extend([
            Step(f"generated-quality-{toolchain}", ("bash", "scripts/python-with-toml.sh", "scripts/check-generated-crates.py", "quality", toolchain), 1800),
            Step(f"repository-consumer-{toolchain}", ("./scripts/check-cargo-consumer.sh", toolchain, "tests/fixtures/repository-consumer/Cargo.toml", "target/repository-consumer")),
            Step(f"full-schema-consumer-{toolchain}", ("./scripts/check-cargo-consumer.sh", toolchain, "tests/fixtures/full-schema/consumer/Cargo.toml", "target/full-schema-corpus", "-p", "chigiri-full-schema-consumer", "-p", "chigiri-fixture-domain")),
            Step(f"fuzz-regression-{toolchain}", (*cargo, "test", "--locked", "--manifest-path", "fuzz/Cargo.toml", "--lib"), 120),
        ])
    # Restore debug preparation after release tests. The first verified debug
    # artifact remains valid; preparation does not replace its success receipt.
    steps.append(Step("chigiri-prepare-contracts-debug-1.95.0", (
        "sh", "tools/standards.sh", "prepare", "--artifact", "chigiri-cli",
        "--toolchain", "1.95.0", "--profile", "debug",
    ), 1800))
    for toolchain in ("1.95.0",):
        steps.append(Step(f"mock-comparison-doctor-{toolchain}", (
            "env", f"RUSTUP_TOOLCHAIN={toolchain}", "python3", "scripts/product_cli.py", "rust",
            "--manifest", "chigiri-mock-blog.toml", "doctor", "--deny-warnings",
        )))
    for script in (
        "check-neutral-endpoint-contract.sh",
        "check-observability-contract.sh",
        "check-usability-contract.sh",
        "check-usability-acceptance-trace.sh",
        "check-rate-boundary-contract.sh",
        "check-dx-rate-acceptance-trace.sh",
        "check-operation-audit-identity-acceptance-trace.sh",
        "check-production-acceptance-trace.sh",
        "check-books-quick-start.sh",
        "check-production-performance-harness.sh",
        "test-release-fuzz-toolchain.sh",
        "test-large-consumer-conformance-gates.sh",
    ):
        steps.append(Step(script.removesuffix(".sh"), (f"./scripts/{script}",)))
    for script in (
        "test-validation.py",
        "test-validation-profiles.py",
        "test-validation-workflows.py",
        "test-standards-language.py",
        "test-required-jobs.py",
        "test-production-acceptance-trace.py",
        "test-cargo-consumer-gates.py",
        "test-production-performance.py",
        "test-large-consumer-diagnostics.py",
    ):
        steps.append(Step(script.removesuffix(".py"), ("python3", f"scripts/{script}")))
    steps.extend([
        Step("standards-tooling", ("python3", "scripts/standards-language.py", "tooling"), 1800),
        Step("large-consumer-conformance", ("./scripts/check-large-consumer-conformance.sh",), 1800),
        Step("dependency-policy-regression", ("bash", "scripts/python-with-toml.sh", "scripts/test-dependency-policy.py")),
        Step("dependency-policy", ("bash", "scripts/python-with-toml.sh", "scripts/dependency_policy.py")),
        Step("graphiql-assets-license", ("python3", "scripts/graphiql_inventory.py", "licenses")),
        Step("diff-whitespace", ("git", "diff", "--check")),
        Step("index-whitespace", ("git", "diff", "--cached", "--check")),
        Step("final-implementation-rules", ("./scripts/check-implementation-rules.sh", "--boundaries-only")),
    ])
    selected = select(profile, steps)
    if profile == "ci":
        # Diagnose native runtime failures before the slower Mock/CLI suites.
        workspace_test = next(step for step in selected if step.label == "workspace-test-1.95.0")
        selected.remove(workspace_test)
        after_build = next(i for i, step in enumerate(selected) if step.label == "chigiri-build") + 1
        selected.insert(after_build, workspace_test)
    return selected
