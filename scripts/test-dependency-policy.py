#!/usr/bin/env python3
"""Ensure SDK exclusions cannot hide source or third-party dependencies."""

import copy
import unittest
from pathlib import Path

from dependency_policy import license_exclusions, wildcard_dependencies


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(__file__).resolve().parent.parent
        self.runtime = self.root / "crates/facade/Cargo.toml"
        self.sdk = self.root / "generated/books/Cargo.toml"
        self.registered = {self.sdk: ("sdk", "0.0.0")}
        self.metadata = {
            "workspace_members": ["runtime"],
            "packages": [
                {"id": "sdk", "name": "sdk", "version": "0.0.0", "source": None,
                 "manifest_path": str(self.sdk), "publish": [],
                 "dependencies": [{"name": "chigiri", "kind": None, "source": None,
                                   "path": str(self.runtime.parent)}]},
                {"id": "runtime", "name": "chigiri", "version": "0.1.0",
                 "source": None, "manifest_path": str(self.runtime)},
            ],
            "resolve": {"nodes": [{"id": "sdk", "deps": [{"name": "chigiri", "pkg": "runtime"}]}]},
        }

    def test_registered_only_and_runtime_remains_root(self):
        self.assertEqual(license_exclusions(self.root, self.metadata, self.registered), ["sdk@0.0.0"])
        self.assertEqual(license_exclusions(self.root, self.metadata, {}), [])

    def test_source_publish_dependency_and_graph_mutations(self):
        mutations = [
            lambda m: m["packages"][0].update(source="registry+https://example.test"),
            lambda m: m["packages"][0].update(publish=None),
            lambda m: m["packages"][0].update(version="0.1.0"),
            lambda m: m["packages"][0]["dependencies"].append({"name": "third-party"}),
            lambda m: m["packages"][0]["dependencies"][0].update(kind="build"),
            lambda m: m["packages"][0]["dependencies"][0].update(source="git+https://example.test"),
            lambda m: m["packages"][0]["dependencies"][0].update(path=str(self.root)),
            lambda m: m.update(workspace_members=[]),
            lambda m: m["workspace_members"].append("sdk"),
            lambda m: m["packages"][1].update(manifest_path=str(self.root / "other/Cargo.toml")),
            lambda m: m["packages"][1].update(source="registry+https://example.test"),
            lambda m: m["resolve"]["nodes"][0]["deps"].append({"name": "other", "pkg": "runtime"}),
            lambda m: m["packages"].append(dict(m["packages"][0], id="other-sdk")),
        ]
        for index, mutate in enumerate(mutations):
            with self.subTest(index=index):
                value = copy.deepcopy(self.metadata)
                mutate(value)
                with self.assertRaises(ValueError):
                    license_exclusions(self.root, value, self.registered)


class WildcardTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {
            "workspace_members": ["a", "b"],
            "packages": [
                {"id": "a", "name": "a", "source": None, "publish": None,
                 "manifest_path": "/fixture/a/Cargo.toml", "dependencies": [
                     {"name": "b", "req": "*", "kind": None, "source": None, "path": "/fixture/b"}]},
                {"id": "b", "name": "b", "source": None, "publish": None,
                 "manifest_path": "/fixture/b/Cargo.toml", "dependencies": []},
            ],
        }

    def test_local_workspace_and_prior_private_dev_exceptions(self):
        self.assertEqual(wildcard_dependencies(self.metadata), 1)
        for update in ({"publish": []}, {}):
            value = copy.deepcopy(self.metadata)
            value["packages"][0].update(update)
            if not update:
                value["packages"][0]["dependencies"][0]["kind"] = "dev"
            value["workspace_members"] = []
            self.assertEqual(wildcard_dependencies(value), 0)

    def test_registry_git_unowned_and_wrong_identity_are_rejected(self):
        mutations = [
            lambda m: m["packages"][0]["dependencies"][0].update(path=None, source="registry+https://example.test"),
            lambda m: m["packages"][0]["dependencies"][0].update(path=None, source="git+https://example.test"),
            lambda m: m.update(workspace_members=["a"]),
            lambda m: m["packages"][1].update(source="registry+https://example.test"),
            lambda m: m["packages"][0]["dependencies"][0].update(path="/fixture/unowned"),
            lambda m: m["packages"][0]["dependencies"][0].update(name="different-package"),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                value = copy.deepcopy(self.metadata)
                mutate(value)
                with self.assertRaises(ValueError):
                    wildcard_dependencies(value)

    def test_transitive_registry_wildcards_are_still_rejected(self):
        value = copy.deepcopy(self.metadata)
        value["packages"].append({"id": "external", "name": "external", "source": "registry+crates.io",
                                  "publish": [], "manifest_path": "/cache/external/Cargo.toml",
                                  "dependencies": [{"name": "other", "req": "*", "kind": None,
                                                    "source": "registry+crates.io"}]})
        with self.assertRaises(ValueError):
            wildcard_dependencies(value)


if __name__ == "__main__":
    unittest.main()
