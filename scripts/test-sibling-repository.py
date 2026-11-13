#!/usr/bin/env python3
"""Ensure removed sibling files, directories, and aliases cannot return."""

import tempfile
import unittest
from pathlib import Path

from sibling_repository import REMOVED, verify


class RepositoryTests(unittest.TestCase):
    def test_current_surface_and_every_removed_path(self):
        repository = Path(__file__).resolve().parent.parent
        verify(repository)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            library = root / "crates/codegen/src/lib.rs"
            library.parent.mkdir(parents=True)
            original = (repository / "crates/codegen/src/lib.rs").read_text()
            library.write_text(original)
            verify(root)
            for name in REMOVED:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("synthetic sibling content\n")
                try:
                    with self.subTest(path=name), self.assertRaises(ValueError):
                        verify(root)
                finally:
                    path.unlink()
            library.write_text(original + "\npub use config::BindingConfig;\n")
            with self.assertRaises(ValueError):
                verify(root)
            library.write_text(original.replace("ResolvedManifest", "LegacyManifest"))
            with self.assertRaises(ValueError):
                verify(root)


if __name__ == "__main__":
    unittest.main()
