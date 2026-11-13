#!/usr/bin/env python3
"""Enforce English repository content with the documented name-origin exception."""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path


EXCLUDED_DIRECTORIES = {".git", "target"}
JAPANESE = re.compile(
    "["
    "\u3005-\u3007"  # Ideographic iteration and iteration-related marks
    "\u303b"  # Vertical ideographic iteration mark
    "\u3040-\u309f"  # Hiragana
    "\u30a0-\u30ff"  # Katakana
    "\u31f0-\u31ff"  # Katakana phonetic extensions
    "\U0001aff0-\U0001afff"  # Kana extended-B
    "\U0001b000-\U0001b16f"  # Kana supplement and extensions
    "\u3400-\u4dbf"  # CJK unified ideographs extension A
    "\u4e00-\u9fff"  # CJK unified ideographs
    "\uf900-\ufaff"  # CJK compatibility ideographs
    "\uff66-\uff9f"  # Half-width Katakana
    "\U00020000-\U0002a6df"  # CJK unified ideographs extension B
    "\U0002a700-\U0002b73f"  # CJK unified ideographs extension C
    "\U0002b740-\U0002b81f"  # CJK unified ideographs extension D
    "\U0002b820-\U0002ceaf"  # CJK unified ideographs extension E
    "\U0002ceb0-\U0002ebef"  # CJK unified ideographs extension F
    "\U0002ebf0-\U0002ee5f"  # CJK unified ideographs extension I
    "\U0002f800-\U0002fa1f"  # CJK compatibility ideographs supplement
    "\U00030000-\U0003134f"  # CJK unified ideographs extension G
    "\U00031350-\U000323af"  # CJK unified ideographs extension H
    "\U000323b0-\U0003347f"  # CJK unified ideographs extension J
    "]"
)
JAPANESE_PUNCTUATION = re.compile(
    "["
    "\u3000-\u3004"  # Ideographic space and common CJK punctuation
    "\u3008-\u3030"  # CJK brackets, marks, and wave dash
    "\u303d-\u303f"  # Part alternation and annotation marks
    "\uff01-\uff0f"  # Full-width ASCII punctuation
    "\uff1a-\uff20"
    "\uff3b-\uff40"
    "\uff5b-\uff65"
    "\ufe10-\ufe19"  # Vertical punctuation forms
    "\ufe30-\ufe4f"  # CJK compatibility punctuation forms
    "]"
)
THIRD_PARTY_CONTENT_PREFIXES = ("crates/graphiql/assets/", "vendor/")
FIRST_PARTY_VENDOR_PATHS = {"vendor/README.md"}
ESCAPE_FIXTURE_PATHS = {
    "scripts/check_repository_language.py",
    "scripts/test_repository_language.py",
}
UNICODE_ESCAPE = re.compile(
    r"\\u\{([0-9a-fA-F]{1,6})\}|\\u([0-9a-fA-F]{4})|\\U([0-9a-fA-F]{8})"
)
# ADR-0043 permits this one literal etymology line in the root README introduction.
BRAND_ETYMOLOGY_LINE = "The name comes from the Japanese \u5951"


def decode_unicode_escapes(value: str) -> str:
    """Decode source-level Unicode escapes without interpreting other escapes."""

    def replace(match: re.Match[str]) -> str:
        digits = next(group for group in match.groups() if group is not None)
        codepoint = int(digits, 16)
        if codepoint > 0x10FFFF or 0xD800 <= codepoint <= 0xDFFF:
            return match.group(0)
        return chr(codepoint)

    return UNICODE_ESCAPE.sub(replace, value)


def repository_files(root: Path):
    tracked = subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        check=False,
        capture_output=True,
    )
    if tracked.returncode == 0:
        for raw_path in sorted(tracked.stdout.split(b"\0")):
            if not raw_path:
                continue
            path = Path(os.fsdecode(raw_path))
            if not EXCLUDED_DIRECTORIES.intersection(path.parts):
                yield root / path
        return

    for directory, names, filenames in os.walk(root):
        names[:] = sorted(
            name for name in names if name not in EXCLUDED_DIRECTORIES
        )
        base = Path(directory)
        for filename in sorted(filenames):
            yield base / filename


def findings(root: Path):
    for path in repository_files(root):
        relative = path.relative_to(root)
        relative_name = relative.as_posix()
        third_party = (
            relative_name.startswith(THIRD_PARTY_CONTENT_PREFIXES)
            and relative_name not in FIRST_PARTY_VENDOR_PATHS
        )
        if not third_party and (
            JAPANESE.search(relative_name)
            or JAPANESE_PUNCTUATION.search(relative_name)
        ):
            yield relative, 0, "<path>"
        try:
            contents = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if third_party:
            continue
        etymology_available = relative_name == "README.md"
        for line_number, line in enumerate(contents.splitlines(), start=1):
            if line.startswith("## "):
                etymology_available = False
            checked = line
            if etymology_available and line == BRAND_ETYMOLOGY_LINE:
                # Mask only the approved literal, never a whole file or decoded escape.
                checked = line.replace("\u5951", "", 1)
                etymology_available = False
            escaped = (
                checked
                if relative_name in ESCAPE_FIXTURE_PATHS
                else decode_unicode_escapes(checked)
            )
            if (
                JAPANESE.search(checked)
                or JAPANESE_PUNCTUATION.search(checked)
                or JAPANESE.search(escaped)
                or JAPANESE_PUNCTUATION.search(escaped)
            ):
                yield relative, line_number, line.strip()


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    matches = list(findings(root))
    if not matches:
        print(
            "Repository language check passed: "
            "no disallowed Japanese text or punctuation found."
        )
        return 0

    print(
        "Repository language check failed: disallowed Japanese text remains in "
        f"{len(matches)} line(s).",
        file=sys.stderr,
    )
    for path, line_number, line in matches:
        print(f"{path}:{line_number}: {line}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
