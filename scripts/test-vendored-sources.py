#!/usr/bin/env python3
"""Ensure ownership inventory and bounded patches cannot hide source changes."""

import contextlib
import io
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from vendored_sources import ROOT, check


class VendorTests(unittest.TestCase):
    def test_fixed_sources_and_limited_manifest_changes_reject_tampering(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            shutil.copy(ROOT / "scripts/vendored-sources.json", root / "scripts")
            shutil.copytree(ROOT / "vendor", root / "vendor")
            with contextlib.redirect_stdout(io.StringIO()):
                check(root=root)
                check(refresh=True, root=root)
                check(root=root)
            self.optimized_check(root, True)
            for directory, license_name in (
                ("stylua-2.5.2", "LICENSE.md"),
                ("full_moon-2.2.0", "README.md"),
            ):
                self.reject_tampering(root, root / "vendor" / directory, license_name)

    def reject_tampering(self, root, package, license_name):
        for name in ("src/lib.rs", "Cargo.toml", "Cargo.toml.upstream", license_name):
            path = package / name
            original = path.read_bytes()
            path.write_bytes(original + b"\n")
            with self.assertRaises((ValueError, KeyError)):
                check(root=root)
            if name == "src/lib.rs":
                self.optimized_check(root, False)
            path.write_bytes(original)
        for kind in ("extra", "missing", "symlink"):
            path = package / "src/lib.rs"
            original = path.read_bytes()
            extra = package / "extra.rs"
            if kind == "extra":
                extra.write_text("// Unregisteredsource\n")
            elif kind == "missing":
                path.unlink()
            else:
                extra.symlink_to(path)
            with self.assertRaises((ValueError, KeyError)):
                check(root=root)
            extra.unlink(missing_ok=True)
            path.write_bytes(original)

    def optimized_check(self, root, success):
        source = (
            "import sys; from pathlib import Path; sys.path.insert(0, str(Path('scripts').resolve())); "
            "from vendored_sources import check; check(root=Path(sys.argv[1]))"
        )
        result = subprocess.run([sys.executable, "-O", "-c", source, str(root)],
                                cwd=ROOT, capture_output=True, timeout=60)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        if not success:
            self.assertIn(b"Invalid", result.stderr)

if __name__ == "__main__":
    unittest.main()
