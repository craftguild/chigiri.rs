#!/usr/bin/env python3
"""Enforce the publishable host crate's narrow dependency topology."""

from __future__ import annotations

from pathlib import Path
import sys
import tomllib


ALLOWED_CONSUMERS = {
    ("chigiri_codegen", "dependencies"),
    ("chigiri_codegen", "build-dependencies"),
    ("chigiri_mock", "dependencies"),
}
HOST_DEPENDENCIES = {"rustix", "serde", "sha2"}
HOST_TARGETS = {
    "cfg(any(target_os = \"linux\", target_os = \"macos\"))": {"signal-hook"},
    "cfg(windows)": {"ctrlc"},
}
HOST_IMPLEMENTATION_PACKAGES = {"ctrlc", "rustix", "signal-hook", "windows-sys"}


def load(path: Path) -> dict:
    with path.open("rb") as source:
        return tomllib.load(source)


def dependency_tables(manifest: dict):
    for name in ("dependencies", "build-dependencies", "dev-dependencies"):
        yield name, manifest.get(name, {})
    for target, values in manifest.get("target", {}).items():
        for name in ("dependencies", "build-dependencies", "dev-dependencies"):
            yield f"target.{target}.{name}", values.get(name, {})


def package_name(key: str, value: object, workspace_dependencies: dict | None = None) -> str:
    if isinstance(value, dict):
        package = value.get("package", key)
        if isinstance(package, str) and package != key:
            return package
        if value.get("workspace") is True and workspace_dependencies is not None:
            inherited = workspace_dependencies.get(key)
            if inherited is not None:
                return package_name(key, inherited)
    return key


def package_dependencies(dependencies: dict, package: str, workspace_dependencies: dict):
    return [
        (key, value)
        for key, value in dependencies.items()
        if package_name(key, value, workspace_dependencies) == package
    ]


def workspace_dependency(manifest: dict, section: str, workspace_dependencies: dict) -> bool:
    dependencies = manifest.get(section, {})
    return package_dependencies(dependencies, "chigiri_host", workspace_dependencies) == [
        ("chigiri_host", {"workspace": True})
    ]


def check(repository: Path) -> list[str]:
    failures: list[str] = []
    root = load(repository / "Cargo.toml")
    workspace = root.get("workspace", {})
    package = workspace.get("package", {})
    members = workspace.get("members", [])
    workspace_dependencies = workspace.get("dependencies", {})
    if "crates/host" not in members or "crates/host" not in workspace.get("default-members", []):
        failures.append("crates/host must be a workspace and default member")
    expected_root = {"path": "crates/host"}
    if workspace_dependencies.get("chigiri_host") != expected_root:
        failures.append("workspace chigiri_host must use the exact local path without a redundant version constraint")
    workspace_host_dependencies = package_dependencies(
        workspace_dependencies, "chigiri_host", workspace_dependencies
    )
    if workspace_host_dependencies != [("chigiri_host", expected_root)]:
        failures.append("workspace may not alias the chigiri_host package")

    host = load(repository / "crates/host/Cargo.toml")
    host_package = host.get("package", {})
    expected_package = {
        "name": "chigiri_host",
        "version": {"workspace": True},
        "edition": {"workspace": True},
        "rust-version": {"workspace": True},
        "license": {"workspace": True},
    }
    for key, value in expected_package.items():
        if host_package.get(key) != value:
            failures.append(f"host package {key} must remain {value!r}")
    if host_package.get("publish") is False:
        failures.append("chigiri_host must remain publishable")
    host_dependencies = host.get("dependencies", {})
    actual_host_dependencies = {
        package_name(key, value, workspace_dependencies)
        for key, value in host_dependencies.items()
    }
    if set(host_dependencies) != HOST_DEPENDENCIES or actual_host_dependencies != HOST_DEPENDENCIES:
        failures.append("host dependencies must be exactly rustix, serde, and sha2")
    if host.get("build-dependencies") or host.get("dev-dependencies"):
        failures.append("host must not add build or development dependencies")
    targets = host.get("target", {})
    if set(targets) != set(HOST_TARGETS):
        failures.append("host target dependency sections changed")
    for target, allowed in HOST_TARGETS.items():
        values = targets.get(target, {})
        dependencies = values.get("dependencies", {})
        actual = {
            package_name(key, value, workspace_dependencies)
            for key, value in dependencies.items()
        }
        if set(values) != {"dependencies"} or set(dependencies) != allowed or actual != allowed:
            failures.append(f"host dependencies for {target} changed")

    for member in members:
        path = repository / member / "Cargo.toml"
        manifest = load(path)
        name = manifest.get("package", {}).get("name", member)
        for section, dependencies in dependency_tables(manifest):
            host_dependencies = package_dependencies(
                dependencies, "chigiri_host", workspace_dependencies
            )
            if not host_dependencies:
                pass
            else:
                base = section.rsplit(".", 1)[-1]
                if (name, base) not in ALLOWED_CONSUMERS:
                    failures.append(f"{name} may not depend on chigiri_host through {section}")
                elif host_dependencies != [("chigiri_host", {"workspace": True})]:
                    failures.append(f"{name} {section} must use chigiri_host.workspace")
            if name != "chigiri_host" and not section.endswith("dev-dependencies"):
                for key, value in dependencies.items():
                    actual = package_name(key, value, workspace_dependencies)
                    if actual in HOST_IMPLEMENTATION_PACKAGES:
                        failures.append(
                            f"{name} may not depend on host implementation package "
                            f"{actual} through {section}"
                        )
    for name, section in sorted(ALLOWED_CONSUMERS):
        member = "crates/codegen" if name == "chigiri_codegen" else "crates/mock"
        manifest = load(repository / member / "Cargo.toml")
        if not workspace_dependency(manifest, section, workspace_dependencies):
            failures.append(f"{name} must retain chigiri_host in {section}")
    return failures


def main() -> int:
    repository = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent
    failures = check(repository)
    for failure in failures:
        print(f"Host manifest violation: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("Host manifest check passed: publication metadata and dependency direction are closed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
