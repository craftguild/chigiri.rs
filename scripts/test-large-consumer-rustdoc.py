"""Reject incomplete public docs and helper leaks using explicit local fixtures."""

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from large_consumer_rustdoc import REQUIRED, validate


class PublicRustdocTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.public = self.root / "chigiri_large_contract"
        self.public.mkdir()
        for name in REQUIRED:
            (self.public / name).write_text("Public trait API", encoding="utf-8")
        self.index = self.root / "search-index.js"
        self.index.write_text("Public search entries", encoding="utf-8")

    def test_current_public_trait_pages_and_both_search_layouts(self):
        (self.public / "index.html").write_text(
            '<a href="../src/chigiri_large_contract/__chigiri/types/enum000.rs.html">Source</a>',
            encoding="utf-8",
        )
        (self.public / "struct.Resolvers.html").write_text(
            "pub fn builder() -> ResolverStage&lt;C, T, E, 0&gt;", encoding="utf-8"
        )
        validate(self.root)
        self.index.unlink()
        self.index = self.root / "search.index" / "crate.js"
        self.index.parent.mkdir()
        self.index.write_text("Public search entries", encoding="utf-8")
        validate(self.root)

    def test_every_required_page_and_search_index_must_exist(self):
        for name in REQUIRED:
            with self.subTest(name=name):
                path = self.public / name
                path.unlink()
                with self.assertRaises(ValueError):
                    validate(self.root)
                path.write_text("Public trait API", encoding="utf-8")
        self.index.unlink()
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_helper_leaks_fail_in_html_and_search_entries(self):
        extra = self.public / "trait.ExtraResolver.html"
        for path in (extra, self.index):
            tokens = ["__Chigiri", "ProjectionSlots", "ProjectionPlan",
                      "ResolverDispatch", "schema_fingerprint", "ArgumentValues"]
            if path == self.index:
                tokens.append("ResolverStage")
            for token in tokens:
                with self.subTest(path=path.name, token=token):
                    path.write_text(token, encoding="utf-8")
                    with self.assertRaises(ValueError):
                        validate(self.root)
            path.write_text("Public API", encoding="utf-8")

    def test_hidden_stage_has_no_public_item_page(self):
        (self.public / "struct.ResolverStage.html").write_text("Public API", encoding="utf-8")
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_retired_projection_directory_is_rejected(self):
        (self.public / "projection").mkdir()
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_unreadable_and_invalid_utf8_inputs_fail(self):
        with patch("large_consumer_rustdoc.read_file", side_effect=PermissionError):
            with self.assertRaises(OSError):
                validate(self.root)
        self.index.write_bytes(b"\xff")
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_required_page_symlink_is_rejected(self):
        page = self.public / "struct.Resolvers.html"
        page.unlink()
        page.symlink_to(self.index)
        with self.assertRaises((OSError, ValueError)):
            validate(self.root)


if __name__ == "__main__":
    unittest.main()
