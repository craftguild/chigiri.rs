"""Inventory authored sources without traversing ignored local build/corpus data."""

import os
import stat
import subprocess
from pathlib import Path

from implementation_rule_io import read_file


EXCLUDED = {".git", "target", "__pycache__", ".beads", ".claude", ".direnv", ".idea", ".vscode",
            ".craftguild", ".private", "node_modules"}


def source_paths(repository):
    root = repository.resolve()
    try:
        observed = subprocess.run(["git", "-C", str(root), "rev-parse", "--show-toplevel"],
                                  capture_output=True, timeout=10)
    except FileNotFoundError:
        observed = None
    if observed is not None and observed.returncode == 0 and Path(os.fsdecode(observed.stdout).strip()).resolve() == root:
        inventory = subprocess.run(
            ["git", "-C", str(root), "ls-files", "--cached", "--others",
             "--exclude-standard", "--deduplicate", "-z"], capture_output=True, timeout=10,
        )
        if inventory.returncode:
            raise ValueError("Could not inventory Git source paths.")
        candidates = [root / os.fsdecode(name) for name in inventory.stdout.split(b"\0") if name]
    elif (root / ".git").exists():
        raise ValueError("Could not resolve this checkout's Git source root.")
    else:
        candidates = []

        def failure(error):
            raise error

        for directory, children, files in os.walk(root, followlinks=False, onerror=failure):
            children[:] = [name for name in children if name not in EXCLUDED]
            if any((Path(directory) / name).is_symlink() for name in children):
                raise ValueError("Directory symlinks are not allowed in the source snapshot.")
            candidates.extend(Path(directory) / name for name in files)
    result = []
    for path in candidates:
        relative = path.relative_to(root)
        if any(part in EXCLUDED for part in relative.parts):
            continue
        try:
            mode = path.lstat().st_mode
        except FileNotFoundError:
            # Tracked deletions are represented by absence, as in a plain tree.
            continue
        if not stat.S_ISREG(mode):
            raise ValueError("Nonregular files are not allowed in the source snapshot.")
        result.append(path)
    return result


def copy_corpus(repository, target, destination):
    from fixed_fuzz_corpus import corpus_inventory

    source = repository.resolve() / "fuzz/corpus" / target
    inventory = corpus_inventory(repository, source_paths(repository))
    if target not in inventory:
        raise ValueError("The selected target has no authored fixed corpus.")
    for path in inventory[target]:
        output = destination / path.relative_to(source)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(read_file(path, 256 * 1024))
