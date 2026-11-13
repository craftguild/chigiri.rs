#!/usr/bin/env python3
"""Reject SDL reduction, binding omission, and corpus extraction drift."""

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from full_schema_corpus import FIXTURE, verify


class CorpusTests(unittest.TestCase):
    def test_self_contained_full_schema(self):
        fixture = Path(__file__).resolve().parent.parent / FIXTURE
        with tempfile.TemporaryDirectory() as directory:
            isolated = Path(directory).resolve() / "corpus"
            shutil.copytree(fixture, isolated)
            result = verify(isolated)
            self.assertEqual(result["schemaFiles"], 11)
            self.assertEqual(result["boundEnums"], 65)
            # ADR-0053 retired projection output; the canonical ABI 8 layout
            # retains 593 Rust files for the unchanged complete source corpus.
            self.assertEqual(result["generatedRustFiles"], 593)
            mutations = [
                ("graphql/schema.graphql", lambda data: data.replace(b"type Query", b"type ReducedQuery")),
                ("chigiri.toml", lambda data: data.replace(b'  "FormatKind",\n', b"")),
                ("domain/src/lib.rs", lambda data: data.replace(b"    Value180,\n", b"")),
                ("source.json", lambda data: json.dumps(dict(json.loads(data), boundEnums=[])).encode()),
            ]
            for name, mutate in mutations:
                path = isolated / name
                original = path.read_bytes()
                changed = mutate(original)
                self.assertNotEqual(changed, original)
                path.write_bytes(changed)
                try:
                    with self.subTest(name=name), self.assertRaises(ValueError):
                        verify(isolated)
                finally:
                    path.write_bytes(original)
            extra = isolated / "graphql/extra.graphql"
            extra.write_text("type Extra { id: ID! }\n")
            with self.assertRaises(ValueError):
                verify(isolated)
            extra.unlink()
            (isolated / "graphql/segment-04.graphql").unlink()
            with self.assertRaises((OSError, ValueError)):
                verify(isolated)


if __name__ == "__main__":
    unittest.main()
