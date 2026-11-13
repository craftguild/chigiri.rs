#!/usr/bin/env python3
"""Prove server exemptions require actual, exclusive test declaration routes."""

import tempfile
import unittest
from pathlib import Path

from usability_server_boundary import violations


class ServerBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.repository = Path(self.temporary.name)
        self.root = self.repository / "crates/facade/src"
        self.root.mkdir(parents=True)

    def tearDown(self):
        self.temporary.cleanup()

    def write(self, name, source):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)

    def test_product_tokens_still_fail(self):
        self.write("lib.rs", "mod worker;\n")
        for token in ("TcpListener", "TcpStream", "Server::bind", "serve_connection",
                      "tokio::signal", "ctrl_c", "graceful_shutdown"):
            self.write("worker.rs", f"fn server() {{ {token}(); }}\n")
            with self.subTest(token=token):
                self.assertTrue(violations(self.repository))

    def test_comments_and_literals_do_not_own_a_server(self):
        self.write("lib.rs", '// TcpListener\nconst NOTE: &str = "serve_connection";\n')
        self.assertEqual(violations(self.repository), [])

    def test_filename_and_orphan_paths_do_not_grant_exemptions(self):
        self.write("lib.rs", "mod tests;\n")
        self.write("tests.rs", "fn server() { TcpListener::bind(); }\n")
        self.assertTrue(violations(self.repository))
        self.write("lib.rs", "")
        self.assertTrue(violations(self.repository))

    def test_transitive_test_route_allows_loopback(self):
        self.write("lib.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("tests.rs", "mod wire;\n")
        self.write("tests/wire.rs", "fn server() { TcpStream::connect(); }\n")
        self.assertEqual(violations(self.repository), [])

    def test_mixed_explicit_production_route_is_not_exempt(self):
        self.write("lib.rs", '#[cfg(test)]\nmod tests;\n#[path = "tests.rs"]\nmod exposed;\n')
        self.write("tests.rs", "fn server() { TcpListener::bind(); }\n")
        self.assertTrue(violations(self.repository))

    def test_removed_gate_does_not_keep_a_cached_exemption(self):
        self.write("lib.rs", "#[cfg(test)]\nmod tests;\n")
        self.write("tests.rs", "fn server() { TcpListener::bind(); }\n")
        self.assertEqual(violations(self.repository), [])
        self.write("lib.rs", "mod tests;\n")
        self.assertTrue(violations(self.repository))


if __name__ == "__main__":
    unittest.main()
