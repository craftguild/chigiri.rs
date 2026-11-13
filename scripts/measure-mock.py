#!/usr/bin/env python3
"""Record Mock builds, project operations, and sampled RSS under cold or cached conditions."""

import concurrent.futures
import argparse
import hashlib
import http.client
import json
import os
import platform
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from validation_evidence import source_snapshot
from mock_reference_measurements import measure_reference
from mock_project_measurements import measure_projects
from mock_hook_measurements import measure_hooks


ROOT = Path(__file__).resolve().parent.parent


def build(directory, label, target=None):
    command = [
        "cargo", "+1.95.0", "build", "--locked", "--release",
        "-p", "chigiri_cli", "--bin", "chigiri",
    ]
    started = time.monotonic()
    with (directory / f"{label}.log").open("wb") as log:
        subprocess.run(command, cwd=ROOT, env=dict(os.environ, CARGO_TARGET_DIR=str(target or directory / "build")),
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=1800)
    return {"argv": command, "seconds": time.monotonic() - started}


def rss(pid):
    result = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True, check=True, timeout=2)
    return int(result.stdout.strip()) * 1024


def operation(port, query, expected_status=200):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=8)
    try:
        connection.request("POST", "/graphql", json.dumps({"query": query}), {"Content-Type": "application/json"})
        response = connection.getresponse()
        result = json.loads(response.read())
        if response.status != expected_status:
            raise ValueError(f"HTTP status {response.status}; expected {expected_status}")
        return result
    finally:
        connection.close()


def sample_operation(pid, port, query, expected_status=200):
    started = time.monotonic()
    samples = [rss(pid)]
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
        request = pool.submit(operation, port, query, expected_status)
        while not request.done():
            samples.append(rss(pid))
            time.sleep(0.005)
        result = request.result(timeout=8)
    samples.append(rss(pid))
    return {"seconds": time.monotonic() - started, "sampled_peak_rss_bytes": max(samples),
            "rss_after_bytes": samples[-1], "samples": len(samples), "response": result}


def server(directory, binary):
    template = (ROOT / "crates/mock/fixtures/mock-v11-ready.toml").read_text()
    original = "local row = ctx.models.User:create(input)\nreturn row"
    if 'format = "chigiri-mock/v11"' not in template or template.count(original) != 1:
        raise ValueError("The v9 fixture does not contain the expected createUser body.")
    body = '''local row = ctx.models.User:create(input)
local user = row
if args.input.name == "memory" then
    local s = "x" for i = 1, 30 do s = s .. s end
elseif args.input.name == "loop" then
    while true do end
end
return user'''
    config = template.replace("127.0.0.1:4000", "127.0.0.1:0").replace(original, body)
    (directory / "mock.toml").write_text(config)
    started = time.monotonic()
    with (directory / "server.stdout").open("wb") as stdout, (directory / "server.stderr").open("wb") as stderr:
        process = subprocess.Popen([str(binary), "mock", "serve", "--config", "mock.toml"], cwd=directory,
                                   stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr)
        try:
            startup_samples = []
            while True:
                if process.poll() is not None:
                    raise ValueError("server exited before listen")
                startup_samples.append(rss(process.pid))
                text = next((line for line in (directory / "server.stderr").read_text().splitlines() if line.startswith("chigiri mock listening http://")), "")
                if text.startswith("chigiri mock listening http://127.0.0.1:"):
                    port = int(text.rsplit(":", 1)[1].split("/", 1)[0])
                    break
                if time.monotonic() - started > 10:
                    raise ValueError("startup watchdog expired")
                time.sleep(0.005)
            result = {"startup_seconds": time.monotonic() - started, "port": port,
                      "startup_sampled_peak_rss_bytes": max(startup_samples), "ready_rss_bytes": rss(process.pid)}
            result["normal"] = sample_operation(process.pid, port, "{getUsers{id name posts{id title author{id}}}}")
            if result["normal"]["response"].get("errors"):
                raise ValueError("Query usually failed.")
            for case in ("memory", "loop"):
                query = 'mutation{createUser(input:{name:"' + case + '"}){id}}'
                result[case] = sample_operation(process.pid, port, query, expected_status=500)
                response = result[case]["response"]
                errors = response.get("errors", [])
                if "data" not in response or response["data"] is not None or len(errors) != 1:
                    raise ValueError(f"{case}: expected a complete request-failure response.")
                code, message = (("MOCK_EXECUTION_LIMIT_EXCEEDED", "Mock execution limit exceeded")
                                 if case == "memory" else ("MOCK_EXECUTION_TIMEOUT", "Mock execution timed out"))
                if errors[0].get("extensions", {}).get("code") != code or errors[0].get("message") != message:
                    raise ValueError(f"{case}: failure cause does not match")
                if case == "memory" and errors[0]["extensions"].get("limit") != "vmMemoryBytes":
                    raise ValueError("Memory case did not report a VM memory-limit failure.")
            recovery = operation(port, 'mutation{createUser(input:{name:"ok"}){id}}')
            if recovery.get("data", {}).get("createUser", {}).get("id") != "User-3":
                raise ValueError("limit failure did not restore store/counter")
            result["recovery"] = recovery
            stopping = time.monotonic()
            process.send_signal(signal.SIGTERM)
            if process.wait(timeout=5) != 0 or "worker joined" not in (directory / "server.stderr").read_text():
                raise ValueError("worker did not join cleanly")
            result["shutdown_seconds"] = time.monotonic() - stopping
            return result
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reuse-target", type=Path, help="Reuse an existing target directory instead of recording a cold build.")
    parser.add_argument("--hooks-only", action="store_true", help="Run only hook comparison and build measurement.")
    args = parser.parse_args()
    (ROOT / "target").mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="mock-project-measure-", dir=ROOT / "target"))
    target = args.reuse_target.resolve() if args.reuse_target else directory / "build"
    if args.reuse_target and (not target.is_dir() or not target.is_relative_to(ROOT / "target")):
        raise ValueError("The existing repository requires a dedicated target directory.")
    result = {"directory": str(directory.relative_to(ROOT)), "platform": platform.platform(),
              "rustc": subprocess.check_output(["rustc", "+1.95.0", "--version", "--verbose"], text=True),
              "source_sha256": source_snapshot(ROOT)["sha256"],
              "rss_note": "PS RSS KiB is converted to bytes. Runtime is 5ms, project start time is a sampling peak with 20ms sleep added to the PS start time, not the true maximum. VM limit is not compiler/Rust store/process RSS limit."}
    label = "cached_build" if args.reuse_target else "cold"
    result["build_target"] = str(target.relative_to(ROOT))
    print(f"{label} target: {target.relative_to(ROOT)}", flush=True)
    result[label] = build(directory, label, target)
    print(f"{label} seconds: {result[label]['seconds']:.3f}", flush=True)
    result["incremental"] = build(directory, "incremental", target)
    result["incremental"]["change"] = "no source change; cached build"
    # Preserve source bytes and update only the entrypoint mtime before measuring
    # the resulting compile/link step.
    os.utime(ROOT / "crates/cli/src/main.rs", None)
    result["entrypoint_rebuild"] = build(directory, "entrypoint-rebuild", target)
    binary = target / "release/chigiri"
    result["binary_bytes"] = binary.stat().st_size
    result["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
    result["hooks"] = measure_hooks(ROOT, directory, binary, target, rss, operation, sample_operation)
    if not args.hooks_only:
        result["runtime"] = server(directory, binary)
        result["reference"] = measure_reference(ROOT, directory, binary)
        result["projects"] = measure_projects(ROOT, directory, binary)
    if source_snapshot(ROOT)["sha256"] != result["source_sha256"]:
        raise ValueError("The source changed during the measurement.")
    # Preserve every historical ADR measurement run and publish this run as the
    # collected summary artifact.
    rendered = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    (directory / "result.json").write_text(rendered)
    output = ROOT / "target/mock-project-measurements.json"
    output.write_text(rendered)
    print(json.dumps(result, ensure_ascii=False, indent=2), flush=True)


if __name__ == "__main__":
    main()
