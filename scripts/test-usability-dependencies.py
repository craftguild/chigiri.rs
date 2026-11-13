#!/usr/bin/env python3
"""Reject server/default leaks without rejecting the opt-in worker contract."""

import copy
import json
import sys
import unittest
from pathlib import Path

from usability_dependencies import validate


METADATA = json.loads(Path(sys.argv.pop(1)).read_text())


def packages(document):
    return {package["name"]: package for package in document["packages"]}


class DependencyTests(unittest.TestCase):
    def test_current_worker_metadata_is_valid(self):
        validate(METADATA)

    def test_server_libraries_remain_forbidden_even_when_optional(self):
        for name in ("axum", "hyper", "hyper-util", "tower"):
            document = copy.deepcopy(METADATA)
            packages(document)["chigiri"]["dependencies"].append(
                {"name": name, "kind": None, "optional": True})
            with self.subTest(dependency=name), self.assertRaises(ValueError):
                validate(document)

    def test_runtime_cannot_be_unconditional_or_activated_by_other_features(self):
        for mutation in ("required", "default", "ungated", "other", "graphiql-alias"):
            document = copy.deepcopy(METADATA)
            facade = packages(document)["chigiri"]
            if mutation == "required":
                next(entry for entry in facade["dependencies"] if entry["name"] == "tokio")["optional"] = False
            elif mutation == "default":
                facade["features"]["default"] = ["subscription-worker"]
            elif mutation == "ungated":
                facade["features"]["subscription-worker"] = []
            elif mutation == "other":
                facade["features"]["other"] = ["dep:tokio"]
            else:
                facade["features"]["graphiql"].append("alias")
                facade["features"]["alias"] = ["subscription-worker"]
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate(document)

    def test_core_and_retired_package_and_description_guards_remain(self):
        for mutation in ("core", "retired", "facade-description", "http-description"):
            document = copy.deepcopy(METADATA)
            current = packages(document)
            if mutation == "core":
                current["chigiri_core"]["dependencies"].append({"name": "serde"})
            elif mutation == "retired":
                document["packages"].append({"name": "chigiri_production"})
            else:
                current["chigiri" if mutation == "facade-description" else "chigiri_http"]["description"] = "changed"
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate(document)


if __name__ == "__main__":
    unittest.main()
