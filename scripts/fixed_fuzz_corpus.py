"""Validate the explicit fixed-input authority shared by replay and long fuzzing."""

import re
from pathlib import Path

from implementation_rule_io import read_file


TARGETS = frozenset({"parser", "sdl_composition", "http_decode", "json_conversion",
                     "persisted_id", "config_parser"})


def corpus_inventory(repository, authored):
    root = repository.resolve() / "fuzz/corpus"
    inventory = {target: [] for target in TARGETS}
    expected = set()
    source = read_file(repository / "fuzz/fixed-corpus.txt", 64 * 1024).decode("ascii")
    for line in source.splitlines():
        parts = line.split("/")
        if (len(parts) != 2 or parts[0] not in TARGETS
                or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", parts[1]) is None):
            raise ValueError("The fixed corpus inventory has an invalid target or name.")
        path = root / parts[0] / parts[1]
        if path in expected:
            raise ValueError("The fixed corpus inventory has a duplicate input.")
        expected.add(path)
        inventory[parts[0]].append(path)
    actual = {path for path in authored if path.is_relative_to(root)}
    if actual != expected or any(not paths for paths in inventory.values()):
        raise ValueError("The authored fixed corpus differs from its explicit inventory.")
    return inventory


if __name__ == "__main__":
    from validation_sources import source_paths
    repository = Path(__file__).resolve().parent.parent
    inventory = corpus_inventory(repository, source_paths(repository))
    print(f"Fixed corpus inventory passed: {sum(map(len, inventory.values()))} inputs, {len(inventory)} targets.")
