#!/usr/bin/env python3
"""Exercise the current CLI workflow in a fresh directory and retain its evidence."""

import argparse
import hashlib
import http.client
import json
import re
import signal
import subprocess
import tempfile
import time
from pathlib import Path
from product_cli import built_cli


ROOT = Path(__file__).resolve().parent.parent
SDL = """type Note { id: ID! title: String! @constraint(minLength: 1) }
type PageInfo { count: Int! }
type Query { notes: [Note!]! info: PageInfo! }
type Mutation { addNote(title: String!): Note! }
"""
BODIES = {
    "query/notes": "return ctx.models.Note:find({})",
    "query/info": "return {count = ctx.models.Note:count({})}",
    "mutation/addNote": "return ctx.models.Note:create({title = args.title})",
}
INTENTIONAL_ERROR = '''        ctx.fail({
            code = "NOT_IMPLEMENTED",
            message = "This operation is not implemented.",
        })'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--work-parent", type=Path, default=ROOT / "target")
    args = parser.parse_args()
    binary = (args.binary or built_cli()).resolve(strict=True)
    work_parent = args.work_parent.resolve(strict=True)
    base = Path(tempfile.mkdtemp(prefix="mock-quickstart-", dir=work_parent))
    steps = []

    def cli(label, *arguments, expected=0):
        started = time.monotonic()
        command = [str(binary), "mock", *arguments]
        result = subprocess.run(command, cwd=base, stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=60)
        (base / (label + ".stdout")).write_bytes(result.stdout)
        (base / (label + ".stderr")).write_bytes(result.stderr)
        if result.returncode != expected:
            raise ValueError(f"{label}: exit={result.returncode}; {result.stderr.decode()}")
        steps.append({"label": label, "argv": command, "exit": result.returncode,
                      "seconds": time.monotonic() - started})
        return result

    (base / "schema.graphql").write_text(SDL)
    cli("init", "init", "--schema", "schema.graphql", "--config-dir", "mock")
    for model in ("Note", "PageInfo"):
        assert (base / f"mock/seeds/{model}.jsonl").read_bytes() == b""
    config = base / "mock/mock.luau"
    config.write_text(config.read_text().replace("127.0.0.1:4000", "127.0.0.1:0"))
    freshness = cli(
        "freshness", "update", "--config-dir", "mock", "--check", "--format", "json",
    )
    report = json.loads(freshness.stdout)
    assert report["format"] == "chigiri-mock-schema-sync/v1"
    assert report["status"] == "in_sync"
    cli("codegen", "codegen", "--config-dir", "mock")
    cli("scaffold-check", "check", "--config-dir", "mock")
    cli("scaffold-export", "export", "--config-dir", "mock", "--output", "scaffold.toml")
    removed = cli(
        "removed-inventory-option",
        "check", "--list-unimplemented", "--config-dir", "mock", expected=2,
    )
    assert removed.stderr == b"chigiri: usage: invalid command, option or value; see --help\n"
    for path, body in BODIES.items():
        module = base / "mock/resolvers" / (path + ".luau")
        source = module.read_text()
        assert source.count(INTENTIONAL_ERROR) == 1
        replacement = "        " + body.replace("\n", "\n        ")
        module.write_text(source.replace(INTENTIONAL_ERROR, replacement))
    cli("authored-check", "check", "--config-dir", "mock")
    (base / "schema.graphql").write_text('"Quick-start public contract"\n' + SDL)
    drift = cli(
        "freshness-drift", "update", "--config-dir", "mock", "--check",
        "--format", "json", expected=1,
    )
    assert json.loads(drift.stdout)["status"] == "update_required"
    adoption = cli("update", "update", "--config-dir", "mock")
    assert b"schema_changed = true" in adoption.stdout
    adopted = cli(
        "freshness-adopted", "update", "--config-dir", "mock", "--check",
        "--format", "json",
    )
    assert json.loads(adopted.stdout)["status"] == "in_sync"
    cli("post-adoption-check", "check", "--config-dir", "mock")
    cli("reference-text", "reference", "all", "--config-dir", "mock")
    reference = cli("reference-json", "reference", "--config-dir", "mock", "--format", "json")
    document = json.loads(reference.stdout)
    assert document["format"] == "chigiri-mock-reference/v13"
    assert document["readiness_checked"] is False
    assert any(rule["id"] == "constraint.regex_dialect" for rule in document["rules"])
    cli("export", "export", "--config-dir", "mock", "--output", "portable.toml")
    # Remove the source project to prove that the portable bundle is self-contained.
    (base / "mock").rename(base / "source-project.saved")
    (base / "schema.graphql").rename(base / "schema.saved")
    observations = []

    def serve(label, mutate):
        stdout_path = base / f"{label}.stdout"
        stderr_path = base / f"{label}.stderr"
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            process = subprocess.Popen(
                [str(binary), "mock", "serve", "--config", "portable.toml"],
                cwd=base, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
            )
        try:
            deadline = time.monotonic() + 20
            while True:
                logs = stderr_path.read_text()
                match = re.search(r"listening http://127\.0\.0\.1:(\d+)/graphql", logs)
                if match:
                    port = int(match[1])
                    break
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError(f"portable server failed: {logs}")
                time.sleep(.02)

            def graphql(query):
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=8)
                try:
                    connection.request("POST", "/graphql", json.dumps({"query": query}),
                                       {"Content-Type": "application/json"})
                    response = connection.getresponse()
                    value = json.loads(response.read())
                    assert response.status == 200 and "errors" not in value, value
                    observations.append({"round": label, "query": query, "response": value})
                    return value["data"]
                finally:
                    connection.close()

            assert graphql("{notes{id title} info{count}}") == {"notes": [], "info": {"count": 0}}
            if mutate:
                created = graphql('mutation{addNote(title:"First note."){id title}}')["addNote"]
                assert re.fullmatch(r"[0-7][0-9a-hjkmnp-tv-z]{25}", created["id"])
                assert created["title"] == "First note."
                assert graphql("{notes{id title} info{count}}") == {
                    "notes": [created], "info": {"count": 1},
                }
            process.send_signal(signal.SIGTERM)
            assert process.wait(timeout=5) == 0
            assert "worker joined" in stderr_path.read_text()
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)

    serve("server", True)
    serve("restart", False)
    try:
        directory = str(base.relative_to(ROOT))
    except ValueError:
        directory = str(base)
    result = {"directory": directory, "steps": steps,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "http": observations}
    (base / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
