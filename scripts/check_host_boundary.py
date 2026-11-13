#!/usr/bin/env python3
"""Reject platform selection and direct host APIs above chigiri_host."""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path
import re
import sys


FORBIDDEN = (
    (re.compile(r"std::os::(?:unix|windows)"), "direct standard-library OS import"),
    (
        re.compile(r"\buse\b[^;]*\bstd\s*::\s*(?:os\b|\{[^;]*\bos\b)[^;]*;"),
        "direct standard-library OS import",
    ),
    (
        re.compile(r"\b(?:use|extern\s+crate)\s+(?:::)?std\s+as\s+\w+[^;]*;"),
        "direct standard-library OS import",
    ),
    (
        re.compile(r"\buse\b[^;]*\bstd\s+as\s+\w+[^;]*;"),
        "direct standard-library OS import",
    ),
    (
        re.compile(r"\buse\b[^;]*\bstd\s*::\s*\{[^;]*\bself\s+as\s+\w+[^;]*;"),
        "direct standard-library OS import",
    ),
    (re.compile(r"\brustix\b"), "direct rustix import"),
    (re.compile(r"\bsignal_hook\b"), "direct signal-hook import"),
    (re.compile(r"\bwindows_sys\b"), "direct Windows binding import"),
    (re.compile(r"\bctrlc\b"), "direct ctrlc import"),
    (
        re.compile(r"tokio::signal::(?:unix|windows)"),
        "direct target-specific Tokio signal import",
    ),
    (
        re.compile(r"\buse\b[^;]*\btokio\s*::\s*(?:signal\b|\{[^;]*\bsignal\b)[^;]*;"),
        "direct target-specific Tokio signal import",
    ),
    (
        re.compile(r"\b(?:use|extern\s+crate)\s+(?:::)?tokio\s+as\s+\w+[^;]*;"),
        "direct target-specific Tokio signal import",
    ),
    (
        re.compile(r"\buse\b[^;]*\btokio\s+as\s+\w+[^;]*;"),
        "direct target-specific Tokio signal import",
    ),
    (
        re.compile(r"\buse\b[^;]*\btokio\s*::\s*\{[^;]*\bself\s+as\s+\w+[^;]*;"),
        "direct target-specific Tokio signal import",
    ),
)
HOST_FORBIDDEN = (
    (
        re.compile(
            r"impl(?:\s*<[^>{;]*>)?\s+"
            r"(?:(?:::)?std\s*::\s*convert\s*::\s*)?AsRef\s*<\s*"
            r"(?:(?:::)?std\s*::\s*path\s*::\s*)?Path\s*>\s+for\s+"
            r"(?:\w+\s*::\s*)*(?:Directory|ResolvedPath)"
        ),
        "native path conversion from a host capability",
    ),
)
FORBID_UNSAFE = re.compile(
    r"^\s*#!\[\s*forbid\s*\(\s*unsafe_code\s*\)\s*\]\s*$", re.MULTILINE
)
TARGET_SELECTOR = re.compile(
    r"\b(?:target_(?:os|arch|env|family|vendor|endian|pointer_width|feature|abi)"
    r"|unix|windows)\b"
)
CFG_START = re.compile(r"#\s*\[\s*cfg(?:_attr)?\s*\(|\bcfg!\s*[\[(]")
RAW_LITERAL = re.compile(r"(?:br|rb|r)(?P<hash>#{0,16})\"")
CHARACTER_LITERAL = re.compile(r"(?:b)?'(?:\\.|[^'\\\n])+'")


def blank_region(characters: list[str], start: int, end: int) -> None:
    for index in range(start, end):
        if characters[index] != "\n":
            characters[index] = " "


def sanitize_rust(text: str) -> str:
    """Blank comments and literals while retaining code positions and line numbers."""
    characters = list(text)
    index = 0
    while index < len(text):
        if text.startswith("//", index):
            end = text.find("\n", index)
            end = len(text) if end < 0 else end
            blank_region(characters, index, end)
            index = end
            continue
        if text.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < len(text) and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            blank_region(characters, index, end)
            index = end
            continue
        raw = RAW_LITERAL.match(text, index)
        if raw:
            closing = '"' + raw.group("hash")
            end = text.find(closing, raw.end())
            end = len(text) if end < 0 else end + len(closing)
            blank_region(characters, index, end)
            index = end
            continue
        quote = index + 1 if text.startswith(('b"', 'c"'), index) else index
        if quote < len(text) and text[quote] == '"':
            end = quote + 1
            while end < len(text):
                if text[end] == "\\":
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
            blank_region(characters, index, min(end, len(text)))
            index = end
            continue
        character = CHARACTER_LITERAL.match(text, index)
        if character:
            end = character.end()
            blank_region(characters, index, end)
            index = end
            continue
        index += 1
    return "".join(characters)


def native_path_aliases(text: str) -> set[str]:
    aliases = {"Path", "PathBuf"}
    changed = True
    while changed:
        changed = False
        names = "|".join(map(re.escape, sorted(aliases)))
        patterns = (
            rf"\btype\s+(\w+)[^=;]*=\s*[^;]*\b(?:{names})\b",
            rf"\buse\s+[^;]*\b(?:{names})\s+as\s+(\w+)\s*;",
        )
        for pattern in patterns:
            for alias in re.findall(pattern, text):
                if alias not in aliases:
                    aliases.add(alias)
                    changed = True
    return aliases


def target_cfg_lines(text: str) -> list[int]:
    lines = []
    pairs = {"(": ")", "[": "]", "{": "}"}
    for found in CFG_START.finditer(text):
        opened = found.end() - 1
        opening = text[opened]
        closing = pairs[opening]
        depth = 1
        cursor = opened + 1
        while cursor < len(text) and depth:
            if text[cursor] == opening:
                depth += 1
            elif text[cursor] == closing:
                depth -= 1
            cursor += 1
        if depth == 0 and TARGET_SELECTOR.search(text[opened + 1:cursor - 1]):
            lines.append(text.count("\n", 0, found.start()) + 1)
    return lines


def host_forbidden_patterns(text: str):
    aliases = "|".join(map(re.escape, sorted(native_path_aliases(text))))
    extraction = re.compile(
        rf"pub\s+(?:(?:async|const|unsafe)\s+|extern(?:\s+\"[^\"]+\")?\s+)*"
        rf"fn\s+\w+"
        rf"\s*(?:<[^>{{;]*>)?\s*\([^)]*\)\s*->"
        rf"[^;{{]*\b(?:{aliases})\b"
    )
    return ((extraction, "public native path extraction"),) + HOST_FORBIDDEN


def parent_module(path: Path, source_root: Path) -> tuple[Path, str] | None:
    """Find the Rust source that declares one out-of-line module."""
    if path == source_root / "lib.rs":
        return None
    if path.name == "mod.rs":
        module = path.parent.name
        container = path.parent.parent
        if container == source_root:
            return source_root / "lib.rs", module
        sibling = container.with_suffix(".rs")
        if sibling.is_file():
            return sibling, module
        parent = container / "mod.rs"
        return (parent, module) if parent.is_file() else None
    module = path.stem
    if path.parent == source_root:
        return source_root / "lib.rs", module
    sibling = path.parent.with_suffix(".rs")
    if sibling.is_file():
        return sibling, module
    parent = path.parent / "mod.rs"
    return (parent, module) if parent.is_file() else None


@lru_cache(maxsize=None)
def source_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


@lru_cache(maxsize=None)
def sanitized_source(path: Path) -> str:
    return sanitize_rust(source_text(path))


@lru_cache(maxsize=None)
def explicit_path_declarations(source_root: Path) -> dict[Path, tuple[tuple[Path, str], ...]]:
    indexed: dict[Path, list[tuple[Path, str]]] = {}
    path_module = re.compile(
        r"(?P<attrs>(?:\s*#\s*\[[^\]]+\]\s*)*)"
        r"(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;"
    )
    for declaring in source_root.rglob("*.rs"):
        raw = source_text(declaring)
        sanitized = sanitized_source(declaring)
        for found in path_module.finditer(raw):
            if re.search(
                r"\bmod\s+\w+\s*;", sanitized[found.start():found.end()]
            ) is None:
                continue
            selected = re.search(
                r"#\s*\[\s*path\s*=\s*\"([^\"]+)\"\s*\]", found.group("attrs")
            )
            if selected:
                target = (declaring.parent / selected.group(1)).resolve()
                indexed.setdefault(target, []).append((declaring, found.group("attrs")))
    return {path: tuple(declarations) for path, declarations in indexed.items()}


def module_declarations(path: Path, source_root: Path) -> list[tuple[Path, str]]:
    declarations: list[tuple[Path, str]] = []
    parent = parent_module(path, source_root)
    if parent is not None:
        declaring, module = parent
        text = sanitized_source(declaring)
        pattern = (
            rf"(?P<attrs>(?:\s*#\s*\[[^\]]+\]\s*)*)"
            rf"(?:pub(?:\([^)]*\))?\s+)?mod\s+{re.escape(module)}\s*;"
        )
        declarations.extend((declaring, found.group("attrs")) for found in re.finditer(pattern, text))
    declarations.extend(explicit_path_declarations(source_root).get(path.resolve(), ()))
    return list(dict.fromkeys(declarations))


def test_source(
    path: Path,
    source_root: Path,
    visiting: frozenset[Path] = frozenset(),
) -> bool:
    """Permit a file only when every declaration route is transitively test-gated."""
    if path in visiting:
        return False
    declarations = module_declarations(path, source_root)
    if not declarations:
        return False
    cfg_test = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
    return all(
        cfg_test.search(attrs) is not None
        or test_source(declaring, source_root, visiting | {path})
        for declaring, attrs in declarations
    )


def rust_sources(repository: Path) -> list[Path]:
    roots = [
        repository / "crates/codegen/src",
        repository / "crates/mock/src",
        repository / "crates/cli/src",
    ]
    paths = [path for root in roots if root.is_dir() for path in root.rglob("*.rs")]
    for crate in ("codegen", "mock", "cli"):
        build = repository / f"crates/{crate}/build.rs"
        if build.is_file():
            paths.append(build)
    return sorted(paths)


def unsafe_sources(repository: Path, product: list[Path]) -> list[Path]:
    host = repository / "crates/host"
    return product + ([path for path in host.rglob("*.rs")] if host.is_dir() else [])


def crate_roots(repository: Path) -> list[Path]:
    roots = [repository / "crates/host/src/lib.rs"]
    host = repository / "crates/host/src"
    roots.extend((host / "main.rs",))
    roots.extend((host / "bin").glob("*.rs"))
    roots.extend((host / "bin").glob("*/main.rs"))
    for crate in ("codegen", "mock", "cli"):
        base = repository / f"crates/{crate}"
        roots.extend((base / "src/lib.rs", base / "src/main.rs", base / "build.rs"))
        roots.extend((base / "src/bin").glob("*.rs"))
        roots.extend((base / "src/bin").glob("*/main.rs"))
    return sorted({path for path in roots if path.is_file()})


def matches(path: Path, patterns) -> list[tuple[int, str]]:
    text = sanitized_source(path)
    found = []
    for pattern, label in patterns:
        for match in pattern.finditer(text):
            line_number = text.count("\n", 0, match.start()) + 1
            found.append((line_number, label))
    return found


def main() -> int:
    repository = Path(__file__).resolve().parent.parent
    product = rust_sources(repository)
    violations = []
    for path in product:
        source_root = next((root for root in (
            repository / "crates/codegen/src",
            repository / "crates/mock/src",
            repository / "crates/cli/src",
        ) if path == root / "lib.rs" or root in path.parents), None)
        if source_root is not None and test_source(path, source_root):
            continue
        for line, label in matches(path, FORBIDDEN):
            violations.append((path, line, label))
        text = sanitized_source(path)
        for line in target_cfg_lines(text):
            violations.append((path, line, "target-platform selection"))
    host = repository / "crates/host/src"
    for path in sorted(host.rglob("*.rs")) if host.is_dir() else []:
        text = sanitized_source(path)
        found = []
        for pattern, label in host_forbidden_patterns(text):
            for match in pattern.finditer(text):
                found.append((text.count("\n", 0, match.start()) + 1, label))
        for line, label in found:
            violations.append((path, line, label))
    for path in crate_roots(repository):
        if FORBID_UNSAFE.search(
            sanitized_source(path)
        ) is None:
            violations.append((path, 1, "missing crate-wide unsafe prohibition"))
    for path, line, label in sorted(set(violations)):
        relative = path.relative_to(repository).as_posix()
        print(f"Host boundary violation: {relative}:{line}: {label}", file=sys.stderr)
    if violations:
        print(f"{len(set(violations))} host boundary violation(s) found.", file=sys.stderr)
        return 1
    print("Host boundary check passed: product source uses chigiri_host backends.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
