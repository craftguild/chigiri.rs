#!/usr/bin/env python3
"""Focused tests for the repository-language scanner."""

import tempfile
import unittest
from pathlib import Path

from check_repository_language import decode_unicode_escapes, findings


class RepositoryLanguageTests(unittest.TestCase):
    def test_allows_the_literal_name_origin_in_the_root_readme_introduction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text(
                "# Chigiri\n\nThe name comes from the Japanese \u5951\n"
                "(chigiri), meaning a pledge or covenant.\n"
                "Japanese IPA: [tɕiɡiɾi].\n\n## Quick start\nEnglish only.\n",
                encoding="utf-8",
            )

            self.assertEqual(list(findings(root)), [])

    def test_rejects_repeated_name_origin_and_other_japanese_in_the_readme(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            line = "The name comes from the Japanese \u5951"
            (root / "README.md").write_text(
                f"{line}\n{line}\n\u5951\n\u65e5\u672c\u8a9e\nEnglish\uff01\n",
                encoding="utf-8",
            )

            self.assertEqual(
                list(findings(root)),
                [
                    (Path("README.md"), 2, line),
                    (Path("README.md"), 3, "\u5951"),
                    (Path("README.md"), 4, "\u65e5\u672c\u8a9e"),
                    (Path("README.md"), 5, "English\uff01"),
                ],
            )

    def test_rejects_modified_or_escaped_name_origin_lines(self):
        lines = [
            "The name comes from the Japanese \u5951\u308a",
            "The name comes from the Japanese \u5951\uff01",
            "Other context: \u5951",
            r"The name comes from the Japanese \u5951",
            r"The name comes from the Japanese \u{5951}",
            r"The name comes from the Japanese \U00005951",
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for line in lines:
                with self.subTest(line=line):
                    (root / "README.md").write_text(line + "\n", encoding="utf-8")
                    self.assertEqual(
                        list(findings(root)), [(Path("README.md"), 1, line)]
                    )

    def test_rejects_the_name_origin_line_in_other_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            line = "The name comes from the Japanese \u5951"
            for name in ("docs/README.md", "source.txt"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(line + "\n", encoding="utf-8")

            self.assertEqual(
                sorted(findings(root)),
                [(Path("docs/README.md"), 1, line), (Path("source.txt"), 1, line)],
            )

    def test_rejects_the_name_origin_line_after_the_introduction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            line = "The name comes from the Japanese \u5951"
            (root / "README.md").write_text(
                f"# Chigiri\n\n## Details\n{line}\n", encoding="utf-8"
            )

            self.assertEqual(list(findings(root)), [(Path("README.md"), 4, line)])

    def test_reports_japanese_with_a_relative_location(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text(
                "English\n\u65e5\u672c\u8a9e\n\uff76\uff80\uff76\uff85\n\u3005\n"
                "\U00020bb7\n\U0001b000\n\U000323b0\n"
            )

            self.assertEqual(
                list(findings(root)),
                [
                    (Path("source.txt"), 2, "\u65e5\u672c\u8a9e"),
                    (Path("source.txt"), 3, "\uff76\uff80\uff76\uff85"),
                    (Path("source.txt"), 4, "\u3005"),
                    (Path("source.txt"), 5, "\U00020bb7"),
                    (Path("source.txt"), 6, "\U0001b000"),
                    (Path("source.txt"), 7, "\U000323b0"),
                ],
            )

    def test_reports_japanese_in_a_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "\u8cc7\u6599.txt"
            path.write_text("English only\n")

            self.assertEqual(
                list(findings(root)),
                [(Path("\u8cc7\u6599.txt"), 0, "<path>")],
            )

    def test_reports_full_width_punctuation_in_a_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "source\uff01.txt"
            path.write_text("English only\n")

            self.assertEqual(
                list(findings(root)),
                [(Path("source\uff01.txt"), 0, "<path>")],
            )

    def test_reports_supplementary_cjk_in_a_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "\U00020bb7.txt"
            path.write_text("English only\n")

            self.assertEqual(
                list(findings(root)),
                [(Path("\U00020bb7.txt"), 0, "<path>")],
            )

    def test_reports_extension_j_in_a_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "\U000323b0.txt"
            path.write_text("English only\n")

            self.assertEqual(
                list(findings(root)),
                [(Path("\U000323b0.txt"), 0, "<path>")],
            )

    def test_skips_excluded_directories_and_non_utf8_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in (".git", "target"):
                path = root / name
                path.mkdir()
                (path / "ignored.txt").write_text("\u65e5\u672c\u8a9e")
            vendor = root / "vendor"
            vendor.mkdir()
            (vendor / "upstream.rs").write_text("\u65e5\u672c\u8a9e")
            (root / "binary.dat").write_bytes(b"\xff\xfe")
            (root / "source.txt").write_text("English only\n")

            self.assertEqual(list(findings(root)), [])

    def test_scans_first_party_vendor_readme(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            vendor = root / "vendor"
            vendor.mkdir()
            (vendor / "README.md").write_text("English\n\u65e5\u672c\u8a9e\n")

            self.assertEqual(
                list(findings(root)),
                [(Path("vendor/README.md"), 2, "\u65e5\u672c\u8a9e")],
            )

    def test_reports_japanese_and_full_width_punctuation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text(
                "English\uff01\nEnglish\uff1f\nEnglish\uff1b\n"
                "English\uff0c\nEnglish\uff0e\nEnglish\u301c\n"
                "English\ufe12\nEnglish\ufe41\n"
            )

            self.assertEqual(
                [line for _, line, _ in findings(root)],
                [1, 2, 3, 4, 5, 6, 7, 8],
            )

    def test_reports_source_level_japanese_unicode_escapes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.rs").write_text(
                r'const VALUES: [&str; 3] = ["\u{65e5}", "\u672c", "\U00008a9e"];'
                "\n"
            )

            self.assertEqual(
                list(findings(root)),
                [
                    (
                        Path("source.rs"),
                        1,
                        r'const VALUES: [&str; 3] = ["\u{65e5}", "\u672c", "\U00008a9e"];',
                    )
                ],
            )

    def test_allows_non_japanese_and_invalid_unicode_escapes(self):
        value = r"\u{00e9} \uD800 \U00110000"
        self.assertEqual(decode_unicode_escapes(value), "é \\uD800 \\U00110000")

    def test_allows_escape_definitions_in_the_scanner_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = root / "scripts/test_repository_language.py"
            fixture.parent.mkdir()
            fixture.write_text(r'fixture = "\u65e5"' + "\n")

            self.assertEqual(list(findings(root)), [])

    def test_preserves_verbatim_third_party_asset_punctuation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            third_party = root / "crates/graphiql/assets"
            third_party.mkdir(parents=True)
            (third_party / "bundle.js").write_text(
                "const text = '\u65e5\u672c\u8a9e\uff01';\n"
            )
            (third_party / "\u65e5\u672c\u8a9e\uff01.js").write_text("third party\n")
            (root / "source.txt").write_text("English only.\n")

            self.assertEqual(list(findings(root)), [])

    def test_allows_full_width_letters_and_digits_in_unicode_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.txt").write_text("\uff33\uff26\uff11\uff12\uff13\n")

            self.assertEqual(list(findings(root)), [])


if __name__ == "__main__":
    unittest.main()
