#!/usr/bin/env python3
"""Fixture for authentication, network isolation, CLI capability, transport, and process recovery tests."""

import hashlib
import json
import os
from pathlib import Path
import sys
import time

BASE = Path(__file__).resolve().parent
MODE = (BASE / "mode.txt").read_text().strip()
ARGS = sys.argv[1:]
EXPECTED = [
    "--ask-for-approval", "never", "exec", "--sandbox", "read-only",
    "--ephemeral", "--skip-git-repo-check", "--json", "--color", "never",
    "--output-schema", "response-schema.json", "--output-last-message",
    "final-message.json", "-",
]
CONFIGURED = EXPECTED[:3] + [
    "--model", "gpt-5.6-sol", "--config",
    'model_reasoning_effort="medium"',
] + EXPECTED[3:]


def emit(value):
    print(json.dumps(value, ensure_ascii=False, separators=(",", ":")), flush=True)


def pause():
    while True:
        time.sleep(0.05)


# Record invocation metadata without persisting prompts, environment values, or authentication files.
with (BASE / "calls.jsonl").open("a") as record:
    record.write(json.dumps({"argv": ARGS, "cwd": os.getcwd(), "pid": os.getpid()}) + "\n")

if MODE == "hang_capability":
    (BASE / "running.json").write_text(json.dumps({"pid": os.getpid()}))
    pause()

if ARGS == ["--version"]:
    print("codex-cli test-0.153.4" if MODE != "bad_version" else "unrecognized-version")
    sys.exit(0)

if "--help" in ARGS:
    if ARGS not in [
        ["--help"], ["exec", "--help"],
        EXPECTED + ["--help"], CONFIGURED + ["--help"],
    ]:
        sys.exit(64)
    flags = " ".join(EXPECTED + ["--model", "--config"])
    if MODE == "missing_capability":
        flags = flags.replace("--ephemeral", "")
    print(flags)
    sys.exit(0)

if ARGS not in [EXPECTED, CONFIGURED]:
    sys.exit(64)
if not Path.cwd().name.startswith("chigiri-mock-codex-"):
    sys.exit(65)
if Path.cwd().stat().st_mode & 0o777 != 0o700:
    sys.exit(65)
schema = Path("response-schema.json").read_bytes()
if not schema or Path("response-schema.json").stat().st_mode & 0o777 != 0o600:
    sys.exit(65)

(BASE / "running.json").write_text(json.dumps({"pid": os.getpid()}))
if MODE == "early_exit":
    sys.exit(1)
if MODE == "authentication":
    print("PRIVATE_TOKEN 401 Unauthorized /private/backend-path", file=sys.stderr)
    sys.exit(1)
if MODE in ["blocked_stdin", "cancel"]:
    pause()
if MODE == "concurrent_pipes":
    sys.stderr.write("private diagnostic " * 65536)
    sys.stderr.flush()
    emit({"type": "thread.started", "thread_id": "fake-thread"})
    emit({"type": "turn.started"})
    emit({"type": "item.completed", "item": {"id": "reasoning", "type": "reasoning", "text": "x" * 1048576}})
if MODE == "stderr_flood":
    sys.stderr.write("PRIVATE_TOKEN" * (3 * 1024 * 1024))
    sys.stderr.flush()
    pause()
if MODE == "oversize_event":
    sys.stdout.write("x" * (8 * 1024 * 1024 + 1))
    sys.stdout.flush()
    pause()

prompt = sys.stdin.buffer.read()
input_record = {
    "bytes": len(prompt), "sha256": hashlib.sha256(prompt).hexdigest(),
    "schema_sha256": hashlib.sha256(schema).hexdigest(),
    "inherited_home": "HOME" in os.environ, "inherited_path": "PATH" in os.environ,
}
(BASE / "input.json").write_text(json.dumps(input_record))
with (BASE / "input-records.jsonl").open("a") as record:
    record.write(json.dumps(input_record) + "\n")

if MODE != "concurrent_pipes":
    emit({"type": "thread.started", "thread_id": "fake-thread"})
    emit({"type": "turn.started"})
if MODE == "partial_forever":
    sys.stdout.write('{"type":"item.completed",')
    sys.stdout.flush()
    pause()
if MODE == "non_utf8":
    sys.stdout.buffer.write(b"\xff\n")
    sys.stdout.buffer.flush()
    sys.exit(0)

if MODE in ("observation", "observation_gate", "mapped_observation", "mapped_demo"):
    print("PRIVATE_STDERR", file=sys.stderr, flush=True)
    for kind in ["reasoning", "command_execution"]:
        emit({"type": "item.completed", "item": {"id": kind, "type": kind, "text": "PRIVATE_INTERNAL", "aggregated_output": "PRIVATE_TOOL"}})
    for kind, text in [("item.started", "Public"), ("item.updated", "Public text\u001b]52;c;ZmFrZQ==\u0007"), ("item.completed", "Public text complete.")]:
        emit({"type": kind, "item": {"id": "public-1", "type": "agent_message", "text": text}})
    if MODE == "observation_gate":
        emit({"type": "item.completed", "item": {"id": "public-2", "type": "agent_message", "text": "PUBLIC_VISIBLE_MARKER"}})
        while not (BASE / "release-public").exists():
            time.sleep(0.01)

final = json.dumps({"candidates": [{"id": "t0000", "content": "candidate", "explanation": "Candidate"}]}, ensure_ascii=False, separators=(",", ":"))
if MODE in ("fixture", "fixture_pause", "observation_gate"):
    final = (BASE / "response.json").read_text()
    if MODE == "fixture_pause":
        time.sleep(0.8)
elif MODE in ("mapped_fixture", "mapped_then_cancel", "mapped_observation", "mapped_demo"):
    # Tests define expected candidate bodies separately; map only opaque target IDs to fixture paths.
    document = json.loads(prompt[prompt.index(b"{\"format\""):])
    if MODE == "mapped_demo":
        # This artificial response supports manual review without imitating real AI work or token streaming.
        for number in range(3):
            emit({"type": "item.completed", "item": {"id": f"demo-{number}", "type": "agent_message", "text": f"FAKE / artificial response for manual review {number + 1}/3. Verify Unicode display and cancellation."}})
            time.sleep(2)
    if MODE == "mapped_observation":
        number = len((BASE / "input-records.jsonl").read_text().splitlines())
        roles = ",".join(sorted({target["kind"] for target in document["targets"]}))
        emit({"type": "item.completed", "item": {"id": "domain", "type": "agent_message", "text": f"DOMAIN_PUBLIC_BATCH_{number} roles={roles}"}})
        while not (BASE / f"release-domain-{number}").exists():
            time.sleep(0.01)
    if MODE == "mapped_then_cancel" and len(document["targets"]) == 1:
        (BASE / "later-batch.json").write_text(json.dumps({"pid": os.getpid()}))
        pause()
    responses = json.loads((BASE / "responses.json").read_text())
    final = json.dumps({"candidates": [
        {"id": target["id"], "content": responses[target["path"]].replace("\n", "\r\n") if MODE == "mapped_observation" and target["kind"] != "seed" else responses[target["path"]], "explanation": "dedicated fixture"}
        for target in document["targets"]]}, ensure_ascii=False, separators=(",", ":"))
emit({"type": "item.completed", "item": {"id": "final", "type": "agent_message", "text": final}})
if MODE == "failed_terminal":
    emit({"type": "turn.failed", "error": {"message": "PRIVATE_TOKEN backend failed"}})
elif MODE != "no_terminal":
    emit({"type": "turn.completed", "usage": {"input_tokens": 104, "output_tokens": 14}})
if MODE != "missing_final":
    value = "different" if MODE == "wrong_final" else final
    if MODE == "oversize_final":
        value = "x" * (4 * 1024 * 1024 + 1)
    Path("final-message.json").write_text(value)
    if MODE == "invalid_final_utf8":
        Path("final-message.json").write_bytes(b"candidate\xff")
if MODE == "cleanup_unknown":
    Path("unowned-file.txt").write_text("Retained test material")
if MODE == "inherited_pipe":
    descendant = os.fork()
    if descendant == 0:
        pause()
    (BASE / "descendant.json").write_text(json.dumps({"pid": descendant}))
    os._exit(0)
sys.exit(1 if MODE == "exit_one" else 0)
