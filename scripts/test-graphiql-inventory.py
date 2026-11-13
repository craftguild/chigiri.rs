"""Verify GraphiQL asset inputs, outputs, and ownership remain unchanged."""

import shutil
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from graphiql_inventory import BASE, ROOT, TOOLS, check
from graphiql_build import license_inventory, validate_audit, validate_imports, verify_outputs


class AssetInventoryTests(unittest.TestCase):
    def test_optimized_mode_keeps_guard_checks_enabled(self):
        result = subprocess.run([sys.executable, "-O", __file__, "BuildGuards"],
                                cwd=ROOT, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_asset_crate_is_rejected_in_a_workspace(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "Cargo.toml").write_text("[workspace]\n")
            with self.assertRaises(ValueError):
                check(root)

    def test_inventory_detects_asset_and_source_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            shutil.copytree(ROOT / BASE, root / BASE)
            (root / "scripts").mkdir()
            for tool in TOOLS:
                shutil.copyfile(ROOT / tool, root / tool)
            original = check(root)
            self.assertGreater(len(original), 0)
            for name in ["assets/app.js", "frontend/entry.mjs", "src/embedded.rs",
                         "dependency-licenses.json", "frontend/license-supplements.json"]:
                path = root / BASE / name
                data = path.read_bytes()
                path.write_bytes(data + b"\n")
                with self.assertRaises(ValueError):
                    check(root)
                path.write_bytes(data)
            path = root / BASE / "assets/unknown.js"
            path.write_text("// Fake.generated header\n")
            with self.assertRaises(ValueError):
                check(root)
            path.unlink()
            path.symlink_to(root / BASE / "assets/app.js")
            with self.assertRaises(ValueError):
                check(root)
            path.unlink()
            path = root / BASE / "frontend/unknown.mjs"
            path.write_text("// inventory unregistered handwriting. \n")
            with self.assertRaises(ValueError):
                check(root)
            path.unlink()
            source = root / BASE / "assets/app.js"
            data = source.read_bytes()
            source.unlink()
            with self.assertRaises(ValueError):
                check(root)
            source.write_bytes(data)
            self.assertEqual(check(root), original)


class BuildGuards(unittest.TestCase):
    def test_vulnerability_audit_and_external_import_policy(self):
        counts = dict.fromkeys(("info", "low", "moderate", "high", "critical", "total"), 0)
        validate_audit({"metadata": {"vulnerabilities": counts}})
        for invalid in (dict(counts, critical=1, total=1), {"total": 0}):
            with self.assertRaises(ValueError):
                validate_audit({"metadata": {"vulnerabilities": invalid}})
        for path, accepted in (("data:image/png;base64,eA==", True), ("https://outside.example/a.js", False)):
            metadata = {"outputs": {"app.js": {"imports": [{"external": True, "path": path}]}}}
            if accepted:
                validate_imports(metadata)
            else:
                with self.assertRaises(ValueError):
                    validate_imports(metadata)

    def test_output_drift_and_untrusted_license_sources_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "app.js").write_bytes(b"expected")
            verify_outputs(root, {"app.js": b"expected"})
            with self.assertRaises(ValueError):
                verify_outputs(root, {"app.js": b"drift"})
            (root / "license-supplements.json").write_text("[]")
            (root / "package-lock.json").write_text(json.dumps({
                "packages": {"node_modules/example": {"resolved": "https://outside.example/a.tgz"}}
            }))
            with self.assertRaises(ValueError):
                license_inventory(root, {"inputs": {}})

    def test_contract_rejects_assets_crate_as_the_consumer(self):
        spec = importlib.util.spec_from_file_location("contract", ROOT / "scripts/check-graphiql-contract.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        metadata = {"resolve": {"nodes": [{"id": "assets"}]},
                    "packages": [{"id": "assets", "name": "chigiri_graphiql"}]}
        result = subprocess.CompletedProcess([], 0, stdout=json.dumps(metadata))
        with patch.object(module, "cargo", return_value=result):
            with self.assertRaises(ValueError):
                module.main()


if __name__ == "__main__":
    unittest.main()
