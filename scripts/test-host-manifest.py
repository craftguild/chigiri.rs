#!/usr/bin/env python3
"""Regression tests for the host manifest topology checker."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest


REPOSITORY = Path(__file__).resolve().parent.parent
CHECKER = REPOSITORY / "scripts/check-host-manifest.py"


class HostManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="chigiri-host-manifest-")
        self.root = Path(self.temporary.name)
        shutil.copy2(REPOSITORY / "Cargo.toml", self.root / "Cargo.toml")
        with (REPOSITORY / "Cargo.toml").open("rb") as source:
            members = tomllib.load(source)["workspace"]["members"]
        for member in members:
            destination = self.root / member
            destination.mkdir(parents=True)
            shutil.copy2(REPOSITORY / member / "Cargo.toml", destination / "Cargo.toml")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_checker(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CHECKER), str(self.root)],
            text=True,
            capture_output=True,
            check=False,
        )

    def replace(self, relative: str, old: str, new: str) -> None:
        path = self.root / relative
        source = path.read_text(encoding="utf-8")
        self.assertIn(old, source)
        path.write_text(source.replace(old, new, 1), encoding="utf-8")

    def test_current_topology_passes(self) -> None:
        self.assertEqual(self.run_checker().returncode, 0)

    def test_host_rejects_runtime_dependency(self) -> None:
        self.replace("crates/host/Cargo.toml", "[dependencies]\n", "[dependencies]\ntokio = \"1\"\n")
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("host dependencies must be exactly", result.stderr)

    def test_host_rejects_dependency_package_alias(self) -> None:
        self.replace(
            "crates/host/Cargo.toml",
            "serde.workspace = true",
            'serde = { package = "tokio", version = "1" }',
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("host dependencies must be exactly", result.stderr)

    def test_framework_rejects_host_dependency(self) -> None:
        self.replace("crates/facade/Cargo.toml", "[dependencies]\n", "[dependencies]\nchigiri_host.workspace = true\n")
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("chigiri may not depend on chigiri_host", result.stderr)

    def test_framework_rejects_renamed_host_dependency(self) -> None:
        self.replace(
            "crates/facade/Cargo.toml",
            "[dependencies]\n",
            '[dependencies]\nhost_alias = { package = "chigiri_host", path = "../host" }\n',
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("chigiri may not depend on chigiri_host", result.stderr)

    def test_framework_rejects_workspace_inherited_host_alias(self) -> None:
        self.replace(
            "Cargo.toml",
            "[workspace.dependencies]\n",
            '[workspace.dependencies]\nhost_alias = { package = "chigiri_host", '
            'path = "crates/host", version = "=0.1.0" }\n',
        )
        self.replace(
            "crates/facade/Cargo.toml",
            "[dependencies]\n",
            "[dependencies]\nhost_alias.workspace = true\n",
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workspace may not alias the chigiri_host package", result.stderr)
        self.assertIn("chigiri may not depend on chigiri_host", result.stderr)

    def test_product_rejects_workspace_inherited_os_dependency_alias(self) -> None:
        self.replace(
            "Cargo.toml",
            "[workspace.dependencies]\n",
            '[workspace.dependencies]\nos_alias = { package = "rustix", version = "1" }\n',
        )
        self.replace(
            "crates/codegen/Cargo.toml",
            "[dependencies]\n",
            "[dependencies]\nos_alias.workspace = true\n",
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("host implementation package rustix", result.stderr)

    def test_allowed_consumer_rejects_renamed_host_dependency(self) -> None:
        self.replace(
            "crates/mock/Cargo.toml",
            "chigiri_host.workspace = true",
            'host_alias = { package = "chigiri_host", path = "../host" }',
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must use chigiri_host.workspace", result.stderr)

    def test_codegen_must_retain_build_dependency(self) -> None:
        self.replace("crates/codegen/Cargo.toml", "[build-dependencies]\n", "[removed-build-dependencies]\n")
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must retain chigiri_host in build-dependencies", result.stderr)

    def test_workspace_dependency_rejects_redundant_version_constraints(self) -> None:
        self.replace(
            "Cargo.toml",
            'chigiri_host = { path = "crates/host" }',
            'chigiri_host = { path = "crates/host", version = "0.1" }',
        )
        result = self.run_checker()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exact local path without a redundant version constraint", result.stderr)


if __name__ == "__main__":
    unittest.main()
